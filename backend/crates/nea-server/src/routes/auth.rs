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

    let (auth_url, _csrf_token) = client
        .authorize_url(CsrfToken::new_random)
        .url();

    info!("OAuth redirect initiated");
    Ok(Redirect::temporary(auth_url.as_str()).into_response())
}

#[derive(Deserialize)]
pub struct CallbackParams {
    pub code: String,
    #[allow(dead_code)]
    pub state: Option<String>,
}

#[tracing::instrument(skip(app_state, params))]
async fn callback(
    State(app_state): State<AppState>,
    Query(params): Query<CallbackParams>,
) -> Result<Response, ApiError> {
    let client = build_oauth_client(&app_state)?;

    let token_result = client
        .exchange_code(AuthorizationCode::new(params.code))
        .request_async(oauth2::reqwest::async_http_client)
        .await
        .map_err(|e| ApiError::Internal(format!("token exchange failed: {e}")))?;

    let access_token = token_result.access_token().secret().clone();

    // EVE SSO v2 returns a JWT access token. Decode payload without verification
    // to extract the "sub" field: "CHARACTER:EVE:{character_id}"
    let (character_id, character_name) = decode_eve_jwt(&access_token)?;

    let expires_at = token_result
        .expires_in()
        .map(|d| chrono::Utc::now() + chrono::Duration::seconds(d.as_secs() as i64))
        .unwrap_or_else(|| chrono::Utc::now() + chrono::Duration::hours(1));

    // We store empty encrypted tokens for now (placeholder - real encryption would use session_secret)
    let access_token_enc = access_token.as_bytes();
    let refresh_token_enc = token_result
        .refresh_token()
        .map(|t| t.secret().as_bytes().to_vec())
        .unwrap_or_default();

    nea_db::upsert_user(
        &app_state.pool,
        character_id,
        &character_name,
        access_token_enc,
        &refresh_token_enc,
        expires_at,
    )
    .await?;

    let session_id = nea_db::create_session(&app_state.pool, character_id).await?;

    info!(character_id, character_name = %character_name, "OAuth callback success");

    let cookie = format!(
        "{SESSION_COOKIE}={session_id}; Path=/; HttpOnly; SameSite=Lax; Max-Age=86400"
    );

    Ok((
        [(SET_COOKIE, cookie)],
        Redirect::temporary("/"),
    )
        .into_response())
}

/// Decode EVE SSO v2 JWT access token to extract character_id and character_name.
/// The JWT payload contains "sub": "CHARACTER:EVE:{character_id}" and "name": "{character_name}".
fn decode_eve_jwt(token: &str) -> Result<(i64, String), ApiError> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return Err(ApiError::Internal("invalid JWT format".to_string()));
    }

    // Decode the payload (second part), which is base64url-encoded
    let payload_bytes = base64::Engine::decode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        parts[1],
    )
    .or_else(|_| {
        base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE, parts[1])
    })
    .map_err(|e| ApiError::Internal(format!("JWT base64 decode failed: {e}")))?;

    let payload: serde_json::Value = serde_json::from_slice(&payload_bytes)
        .map_err(|e| ApiError::Internal(format!("JWT JSON parse failed: {e}")))?;

    // "sub" field: "CHARACTER:EVE:12345678"
    let sub = payload["sub"]
        .as_str()
        .ok_or_else(|| ApiError::Internal("missing sub in JWT".to_string()))?;

    let character_id: i64 = sub
        .strip_prefix("CHARACTER:EVE:")
        .ok_or_else(|| ApiError::Internal(format!("unexpected sub format: {sub}")))?
        .parse()
        .map_err(|e| ApiError::Internal(format!("invalid character_id in sub: {e}")))?;

    let character_name = payload["name"]
        .as_str()
        .unwrap_or("Unknown")
        .to_string();

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

    let clear_cookie =
        format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0");

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
    use base64::Engine;

    fn make_jwt(payload_json: &str) -> String {
        let header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(payload_json);
        format!("{header}.{payload}.fake_signature")
    }

    #[test]
    fn test_decode_eve_jwt_valid() {
        let token = make_jwt(r#"{"sub":"CHARACTER:EVE:12345678","name":"Test Pilot"}"#);
        let (id, name) = decode_eve_jwt(&token).unwrap();
        assert_eq!(id, 12345678);
        assert_eq!(name, "Test Pilot");
    }

    #[test]
    fn test_decode_eve_jwt_invalid_format() {
        let result = decode_eve_jwt("not.enough");
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_eve_jwt_missing_sub() {
        let token = make_jwt(r#"{"name":"Test Pilot"}"#);
        let result = decode_eve_jwt(&token);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_eve_jwt_bad_sub_prefix() {
        let token = make_jwt(r#"{"sub":"USER:12345","name":"Test"}"#);
        let result = decode_eve_jwt(&token);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_eve_jwt_missing_name_defaults() {
        let token = make_jwt(r#"{"sub":"CHARACTER:EVE:99999"}"#);
        let (id, name) = decode_eve_jwt(&token).unwrap();
        assert_eq!(id, 99999);
        assert_eq!(name, "Unknown");
    }
}
