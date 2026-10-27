use axum::{Json, extract::State, response::IntoResponse};

use super::ApiState;
use crate::{
    api::{EmptyJsonResponse, Result},
    limits::LimitsConfig,
};

pub async fn h_update_session_ratelimits(
    State(state): State<ApiState>,
    Json(limits): Json<LimitsConfig>,
) -> Result<impl IntoResponse> {
    state
        .api
        .session()
        .ratelimits
        .set_upload_bps(limits.upload_bps);
    state
        .api
        .session()
        .ratelimits
        .set_download_bps(limits.download_bps);
    Ok(Json(EmptyJsonResponse {}))
}

pub async fn h_get_session_ratelimits(State(state): State<ApiState>) -> Result<impl IntoResponse> {
    let config = state.api.session().ratelimits.get_config();
    Ok(Json(config))
}

// Mutations live in the protected, writable router alongside rate limits.
pub async fn h_get_queue(State(state): State<ApiState>) -> Json<crate::QueueConfig> {
    Json(state.api.session().queue_config())
}
pub async fn h_set_queue(
    State(state): State<ApiState>,
    Json(config): Json<crate::QueueConfig>,
) -> Result<Json<EmptyJsonResponse>> {
    config.validate().map_err(|_| {
        crate::ApiError::from((http::StatusCode::BAD_REQUEST, "invalid queue configuration"))
    })?;
    state
        .api
        .session()
        .set_queue_config(config)
        .await
        .map_err(|_| {
            crate::ApiError::from((
                http::StatusCode::CONFLICT,
                "could not update queue configuration",
            ))
        })?;
    Ok(Json(EmptyJsonResponse {}))
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReorderRequest {
    hashes: Vec<librqbit_core::Id20>,
    operation: crate::QueueOperation,
}
pub async fn h_reorder_queue(
    State(state): State<ApiState>,
    Json(request): Json<ReorderRequest>,
) -> Result<Json<EmptyJsonResponse>> {
    if request.hashes.is_empty() || request.hashes.len() > 128 {
        return Err(crate::ApiError::from((
            http::StatusCode::BAD_REQUEST,
            "invalid number of torrent hashes",
        )));
    }
    state
        .api
        .session()
        .reorder_queue(request.hashes, request.operation)
        .await
        .map_err(|_| {
            crate::ApiError::from((
                http::StatusCode::CONFLICT,
                "could not reorder torrent queue",
            ))
        })?;
    Ok(Json(EmptyJsonResponse {}))
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForceStartRequest {
    value: bool,
}
pub async fn h_force_start(
    State(state): State<ApiState>,
    axum::extract::Path(id): axum::extract::Path<crate::api::TorrentIdOrHash>,
    Json(request): Json<ForceStartRequest>,
) -> Result<Json<EmptyJsonResponse>> {
    state
        .api
        .session()
        .set_torrent_force_start(id, request.value)
        .await
        .map_err(|_| {
            crate::ApiError::from((http::StatusCode::CONFLICT, "could not change force start"))
        })?;
    if request.value {
        state.api.api_torrent_action_start(id).await?;
    }
    state.api.session().reconcile_queue().await.map_err(|_| {
        crate::ApiError::from((http::StatusCode::CONFLICT, "could not reconcile queue"))
    })?;
    Ok(Json(EmptyJsonResponse {}))
}
