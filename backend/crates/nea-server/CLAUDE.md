# nea-server

Axum HTTP API server — serves all `/api` routes. Routed through Traefik in production.

## Responsibilities

- REST API endpoints for dashboard, items, market data, analysis results, destruction data, and auth
- EVE SSO OAuth2 login flow
- Session management via HttpOnly cookies (24h expiry)
- Request ID middleware (UUID v4 in `x-request-id` header)

## Key Types

- `AppState` — PgPool, ESI OAuth config, session_secret, analysis_running flag
- `ApiError` — Db, NotFound, BadRequest, Unauthorized, Internal (implements IntoResponse)

## Route Modules

| Module | Endpoints |
|--------|-----------|
| `dashboard` | `GET /api/dashboard`, `GET /api/dashboard/movers` |
| `items` | `GET /api/items` (search), `GET /api/items/:type_id` |
| `market` | `GET /api/market/:type_id/history`, `GET /api/market/:type_id/snapshots` |
| `analysis` | `GET /api/analysis/:type_id/correlations`, `GET /api/analysis/:type_id/lag`, `GET /api/analysis/top`, `POST /api/analysis/run`, `GET /api/analysis/status` |
| `destruction` | `GET /api/destruction/:type_id` |
| `auth` | `GET /api/auth/login`, `GET /api/auth/callback`, `POST /api/auth/logout`, `GET /api/auth/me` |

## Health Check

`GET /api/health` — returns 200 with `{"status": "ok"}`

## Middleware Stack

- `x-request-id` UUID v4 on all responses
- Permissive CORS (any origin, any method, any header)
- `TraceLayer` at INFO level

## Auth Flow

1. `GET /api/auth/login` → redirect to EVE SSO
2. `GET /api/auth/callback` → exchange code for token → decode JWT payload (no signature verification) → extract character_id/name → upsert user → set session cookie
3. Session validated on protected routes via cookie lookup

## Patterns

- Handlers use `#[tracing::instrument(skip(state))]`
- Return `Result<Json<T>, ApiError>` with From conversions from DbError

## Dependencies

External: axum, axum-extra (cookies), tokio, tower-http, reqwest, serde, chrono, uuid, tracing, oauth2, base64, thiserror
Workspace: nea-db, nea-esi, nea-analysis
