use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use tracing::debug;

use crate::error::ApiError;
use crate::state::AppState;
use nea_db::KillmailDetail;

pub fn routes() -> Router<AppState> {
    Router::new().route("/killmails/{killmail_id}", get(get_killmail))
}

#[tracing::instrument(skip(state))]
async fn get_killmail(
    State(state): State<AppState>,
    Path(killmail_id): Path<i64>,
) -> Result<Json<KillmailDetail>, ApiError> {
    let killmail = nea_db::get_killmail_by_id(&state.pool, killmail_id)
        .await?
        .ok_or_else(|| ApiError::NotFound(format!("killmail_id {killmail_id} not found")))?;

    let (victim, attackers, items) = tokio::try_join!(
        nea_db::get_killmail_victim_detail(&state.pool, killmail_id, killmail.kill_time),
        nea_db::get_killmail_attackers_detail(&state.pool, killmail_id, killmail.kill_time),
        nea_db::get_killmail_items_detail(&state.pool, killmail_id, killmail.kill_time),
    )?;

    let victim = victim.ok_or_else(|| {
        ApiError::NotFound(format!("victim for killmail_id {killmail_id} not found"))
    })?;

    debug!(
        killmail_id,
        attackers = attackers.len(),
        items = items.len(),
        "get_killmail"
    );
    Ok(Json(KillmailDetail {
        killmail,
        victim,
        attackers,
        items,
    }))
}
