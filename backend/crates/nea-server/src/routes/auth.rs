use axum::{
    extract::{Query, State},
    http::header::SET_COOKIE,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
    Json, Router,
};
use axum_extra::extract::CookieJar;
use oauth2::{
    basic::BasicClient, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken,
    RedirectUrl, TokenResponse, TokenUrl,
};
use serde::Deserialize;
use serde_json::json;

use tracing::{debug, info};

use crate::error::ApiError;
use crate::state::AppState;

const EVE_AUTH_URL: &str = "https://login.eveonline.com/v2/oauth/authorize";
const EVE_TOKEN_URL: &str = "https://login.eveonline.com/v2/oauth/token";
const SESSION_COOKIE: &str = "nea_session";

fn cookie_str(name: &str, value: &str, max_age: i64, secure: bool) -> String {
    let mut c = format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}");
    if secure {
        c.push_str("; Secure");
    }
    c
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
}

fn build_oauth_client(state: &AppState) -> Result<BasicClient, ApiError> {
    let client = BasicClient::new(
        ClientId::new(state.esi_client_id.clone()),
        Some(ClientSecret::new(state.esi_client_secret.clone())),
        AuthUrl::new(EVE_AUTH_URL.to_string())
            .map_err(|e| ApiError::Internal(format!("invalid auth url: {e}")))?,
        Some(
            TokenUrl::new(EVE_TOKEN_URL.to_string())
                .map_err(|e| ApiError::Internal(format!("invalid token url: {e}")))?,
        ),
    )
    .set_redirect_uri(
        RedirectUrl::new(state.esi_callback_url.clone())
            .map_err(|e| ApiError::Internal(format!("invalid redirect url: {e}")))?,
    );

    Ok(client)
}

#[tracing::instrument(skip(state))]
async fn login(State(state): State<AppState>) -> Result<Response, ApiError> {
    let client = build_oauth_client(&state)?;

    let (auth_url, csrf_token) = client
        .authorize_url(CsrfToken::new_random)
        .url();

    let csrf_cookie = cookie_str("nea_csrf", csrf_token.secret(), 600, state.secure_cookies);

    info!("OAuth redirect initiated");
    Ok((
        [(SET_COOKIE, csrf_cookie)],
        Redirect::temporary(auth_url.as_str()),
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct CallbackParams {
    pub code: String,
    pub state: String,
}

#[tracing::instrument(skip(app_state, params, jar))]
async fn callback(
    State(app_state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<CallbackParams>,
) -> Result<Response, ApiError> {
    // Validate CSRF token
    let csrf_cookie = jar
        .get("nea_csrf")
        .ok_or_else(|| ApiError::BadRequest("missing CSRF cookie".to_string()))?;
    if csrf_cookie.value() != params.state {
        return Err(ApiError::BadRequest("CSRF validation failed".to_string()));
    }

    let client = build_oauth_client(&app_state)?;

    let token_result = client
        .exchange_code(AuthorizationCode::new(params.code))
        .request_async(oauth2::reqwest::async_http_client)
        .await
        .map_err(|e| ApiError::Internal(format!("token exchange failed: {e}")))?;

    let access_token = token_result.access_token().secret().clone();

    let (character_id, character_name) = verify_eve_jwt(&access_token, &app_state).await?;

    let expires_at = token_result
        .expires_in()
        .map(|d| chrono::Utc::now() + chrono::Duration::seconds(d.as_secs() as i64))
        .unwrap_or_else(|| chrono::Utc::now() + chrono::Duration::hours(1));

    // Tokens are unused — store empty bytes. Add encryption if retrieval is ever needed.
    nea_db::upsert_user(
        &app_state.pool,
        character_id,
        &character_name,
        &[],
        &[],
        expires_at,
    )
    .await?;

    let session_id = nea_db::create_session(&app_state.pool, character_id).await?;

    info!(character_id, character_name = %character_name, "OAuth callback success");

    let session_cookie = cookie_str(SESSION_COOKIE, &session_id.to_string(), 86400, app_state.secure_cookies);
    let clear_csrf = cookie_str("nea_csrf", "", 0, app_state.secure_cookies);

    let mut response = Redirect::temporary("/").into_response();
    response.headers_mut().append(SET_COOKIE, session_cookie.parse().unwrap());
    response.headers_mut().append(SET_COOKIE, clear_csrf.parse().unwrap());
    Ok(response)
}

const EVE_JWKS_URL: &str = "https://login.eveonline.com/oauth/jwks";

#[derive(Debug, Deserialize)]
struct EveClaims {
    sub: String,
    name: Option<String>,
}

async fn fetch_jwks(client: &reqwest::Client) -> Result<jsonwebtoken::jwk::JwkSet, ApiError> {
    client
        .get(EVE_JWKS_URL)
        .send()
        .await
        .map_err(|e| ApiError::Internal(format!("JWKS fetch failed: {e}")))?
        .json()
        .await
        .map_err(|e| ApiError::Internal(format!("JWKS parse failed: {e}")))
}

async fn verify_eve_jwt(token: &str, state: &AppState) -> Result<(i64, String), ApiError> {
    let header = jsonwebtoken::decode_header(token)
        .map_err(|e| ApiError::Internal(format!("JWT header decode failed: {e}")))?;
    let kid = header
        .kid
        .ok_or_else(|| ApiError::Internal("JWT missing kid".to_string()))?;

    let client = reqwest::Client::new();

    // Try cached JWKS first, fetch if miss or kid not found
    let find_jwk = |jwks: &jsonwebtoken::jwk::JwkSet, kid: &str| -> Option<jsonwebtoken::jwk::Jwk> {
        jwks.keys.iter().find(|k| k.common.key_id.as_deref() == Some(kid)).cloned()
    };

    let jwk = {
        let cache = state.jwks_cache.read().await;
        cache.as_ref().and_then(|jwks| find_jwk(jwks, &kid))
    };

    let jwk = match jwk {
        Some(jwk) => jwk,
        None => {
            let jwks = fetch_jwks(&client).await?;
            let found = find_jwk(&jwks, &kid)
                .ok_or_else(|| ApiError::Internal(format!("kid {kid} not found in JWKS")))?;
            *state.jwks_cache.write().await = Some(jwks);
            found
        }
    };

    let decoding_key = jsonwebtoken::DecodingKey::from_jwk(&jwk)
        .map_err(|e| ApiError::Internal(format!("failed to build decoding key: {e}")))?;

    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
    validation.set_issuer(&["login.eveonline.com"]);
    validation.set_audience(&[&state.esi_client_id]);

    let token_data = jsonwebtoken::decode::<EveClaims>(token, &decoding_key, &validation)
        .map_err(|e| ApiError::Internal(format!("JWT verification failed: {e}")))?;

    let claims = token_data.claims;

    let character_id: i64 = claims
        .sub
        .strip_prefix("CHARACTER:EVE:")
        .ok_or_else(|| ApiError::Internal(format!("unexpected sub format: {}", claims.sub)))?
        .parse()
        .map_err(|e| ApiError::Internal(format!("invalid character_id in sub: {e}")))?;

    let character_name = claims.name.unwrap_or_else(|| "Unknown".to_string());

    Ok((character_id, character_name))
}

#[tracing::instrument(skip(state, jar))]
async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Response, ApiError> {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        if let Ok(session_id) = cookie.value().parse::<uuid::Uuid>() {
            let _ = nea_db::delete_session(&state.pool, session_id).await;
        }
    }

    info!("user logged out");

    let clear_cookie = cookie_str(SESSION_COOKIE, "", 0, state.secure_cookies);

    Ok((
        [(SET_COOKIE, clear_cookie)],
        Json(json!({"ok": true})),
    )
        .into_response())
}

#[tracing::instrument(skip(state, jar))]
async fn me(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<serde_json::Value>, ApiError> {
    let session_cookie = jar.get(SESSION_COOKIE).ok_or(ApiError::Unauthorized)?;
    let session_id: uuid::Uuid = session_cookie
        .value()
        .parse()
        .map_err(|_| ApiError::Unauthorized)?;

    let session = nea_db::get_session(&state.pool, session_id)
        .await?
        .ok_or(ApiError::Unauthorized)?;

    let user = nea_db::get_user(&state.pool, session.character_id)
        .await?
        .ok_or(ApiError::Unauthorized)?;

    debug!(character_id = user.character_id, "me");
    Ok(Json(json!({
        "character_id": user.character_id,
        "character_name": user.character_name,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eve_claims_deserialize() {
        let json = r#"{"sub":"CHARACTER:EVE:12345678","name":"Test Pilot","iss":"login.eveonline.com"}"#;
        let claims: EveClaims = serde_json::from_str(json).unwrap();
        assert_eq!(claims.sub, "CHARACTER:EVE:12345678");
        assert_eq!(claims.name.as_deref(), Some("Test Pilot"));
    }

    #[test]
    fn test_eve_claims_missing_name() {
        let json = r#"{"sub":"CHARACTER:EVE:99999","iss":"login.eveonline.com"}"#;
        let claims: EveClaims = serde_json::from_str(json).unwrap();
        assert_eq!(claims.sub, "CHARACTER:EVE:99999");
        assert!(claims.name.is_none());
    }

    #[tokio::test]
    async fn test_verify_eve_jwt_rejects_tampered_token() {
        use base64::Engine;
        use jsonwebtoken::{encode, EncodingKey, Header};
        use rsa::pkcs8::EncodePrivateKey;
        use rsa::traits::PublicKeyParts;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        // Generate a test RSA keypair
        let mut rng = rsa::rand_core::OsRng;
        let private_key = rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let pem = private_key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF).unwrap();
        let encoding_key = EncodingKey::from_rsa_pem(pem.as_bytes()).unwrap();

        let mut header = Header::new(jsonwebtoken::Algorithm::RS256);
        header.kid = Some("test-kid".to_string());

        let claims = serde_json::json!({
            "sub": "CHARACTER:EVE:12345678",
            "name": "Test Pilot",
            "iss": "login.eveonline.com",
            "aud": "EVE-app-client-id",
            "exp": chrono::Utc::now().timestamp() + 3600,
        });

        let token = encode(&header, &claims, &encoding_key).unwrap();

        // Tamper with the token by modifying a character in the signature
        let mut tampered = token.clone();
        let last = tampered.pop().unwrap();
        tampered.push(if last == 'A' { 'B' } else { 'A' });

        // Build a JwkSet from the public key
        let public_key = rsa::RsaPublicKey::from(&private_key);
        let n = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(public_key.n().to_bytes_be());
        let e = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(public_key.e().to_bytes_be());

        let jwks_json = serde_json::json!({
            "keys": [{
                "kty": "RSA",
                "use": "sig",
                "kid": "test-kid",
                "alg": "RS256",
                "n": n,
                "e": e,
            }]
        });
        let jwks: jsonwebtoken::jwk::JwkSet = serde_json::from_value(jwks_json).unwrap();

        let state = AppState {
            pool: sqlx::PgPool::connect_lazy("postgres://fake").unwrap(),
            esi_client_id: "EVE-app-client-id".to_string(),
            esi_client_secret: String::new(),
            esi_callback_url: String::new(),
            session_secret: String::new(),
            domain: "localhost".to_string(),
            secure_cookies: false,
            analysis_running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            jwks_cache: Arc::new(RwLock::new(Some(jwks))),
        };

        // Valid token should succeed
        let result = verify_eve_jwt(&token, &state).await;
        assert!(result.is_ok());
        let (id, name) = result.unwrap();
        assert_eq!(id, 12345678);
        assert_eq!(name, "Test Pilot");

        // Tampered token should fail
        let result = verify_eve_jwt(&tampered, &state).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_cookie_str_without_secure() {
        let c = cookie_str("name", "val", 600, false);
        assert!(c.contains("name=val"));
        assert!(c.contains("Max-Age=600"));
        assert!(!c.contains("Secure"));
    }

    #[test]
    fn test_cookie_str_with_secure() {
        let c = cookie_str("name", "val", 600, true);
        assert!(c.contains("; Secure"));
    }
}
