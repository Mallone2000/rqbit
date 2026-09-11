pub(super) mod auth;
mod dto;

use std::{collections::BTreeMap, sync::Arc};

use anyhow::Context;
use axum::{
    Form, Json, Router,
    body::to_bytes,
    extract::{DefaultBodyLimit, FromRequest, Multipart, Query, Request, State},
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use http::{StatusCode, header::CONTENT_TYPE};
use serde::Deserialize;
use tracing::warn;

use self::dto::{Category, Preferences, TorrentFile, TorrentInfo, TorrentProperties};
use super::HttpApi;
use crate::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, TorrentAutomationMetadata,
    api::TorrentIdOrHash,
    session::{PendingAutomationTorrentState, unix_time_seconds},
    torrent_state::TorrentStatsState,
};

type ApiState = Arc<HttpApi>;
type QbitResult<T> = Result<T, QbitError>;

const WEB_API_VERSION: &str = "2.8.1";
const UNKNOWN_ETA_SECONDS: u64 = 8_640_000;
const MAX_LOGIN_BODY_SIZE: usize = 8 * 1024;
const MAX_MAGNET_URL_SIZE: usize = 16 * 1024;

#[derive(Debug)]
struct QbitError {
    status: StatusCode,
    message: String,
}

impl QbitError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn internal(status: StatusCode, message: &'static str, error: impl std::fmt::Debug) -> Self {
        warn!(
            ?error,
            operation = message,
            "qBittorrent API operation failed"
        );
        Self {
            status,
            message: message.to_owned(),
        }
    }
}

impl IntoResponse for QbitError {
    fn into_response(self) -> Response {
        (self.status, self.message).into_response()
    }
}

pub(super) fn make_api_router(state: ApiState) -> Router {
    let mut protected = Router::new()
        .route("/auth/logout", post(auth::logout))
        .route("/app/webapiVersion", get(web_api_version))
        .route("/app/version", get(app_version))
        .route("/app/preferences", get(preferences))
        .route("/torrents/info", get(torrents_info))
        .route("/torrents/properties", get(torrent_properties))
        .route("/torrents/files", get(torrent_files))
        .route("/torrents/categories", get(categories));

    if !state.opts.read_only {
        protected = protected
            .route("/torrents/add", post(add_torrent))
            .route("/torrents/delete", post(delete_torrents))
            .route("/torrents/createCategory", post(create_category))
            .route("/torrents/removeCategories", post(remove_categories))
            .route("/torrents/setCategory", post(set_category))
            .route("/torrents/setShareLimits", post(set_share_limits))
            .route("/torrents/pause", post(pause_torrents))
            .route("/torrents/resume", post(resume_torrents))
            .route("/torrents/topPrio", post(top_priority))
            .route("/torrents/setForceStart", post(set_force_start));
    }

    protected = protected.route_layer(middleware::from_fn_with_state(
        state.clone(),
        auth::require_auth,
    ));

    Router::new()
        .route(
            "/auth/login",
            post(auth::login).layer(DefaultBodyLimit::max(MAX_LOGIN_BODY_SIZE)),
        )
        .merge(protected)
        .layer(middleware::from_fn(auth::require_same_origin))
        .layer(DefaultBodyLimit::max(
            state.opts.max_upload_body_size.unwrap_or(10 * 1024 * 1024),
        ))
        .with_state(state)
}

async fn web_api_version() -> impl IntoResponse {
    WEB_API_VERSION
}

async fn app_version() -> impl IntoResponse {
    format!("v{}", crate::version())
}

async fn preferences(State(state): State<ApiState>) -> Json<Preferences> {
    Json(Preferences {
        save_path: state
            .api
            .session()
            .output_folder()
            .to_string_lossy()
            .into_owned(),
        dht: state.api.session().is_dht_enabled(),
        queueing_enabled: false,
        max_ratio_enabled: false,
        max_ratio: -1.0,
        max_ratio_act: 0,
        max_seeding_time_enabled: false,
        max_seeding_time: 0,
        max_inactive_seeding_time_enabled: false,
        max_inactive_seeding_time: 0,
    })
}

#[derive(Default, Deserialize)]
struct InfoQuery {
    #[serde(default)]
    category: String,
}

async fn torrents_info(
    State(state): State<ApiState>,
    Query(query): Query<InfoQuery>,
) -> Json<Vec<TorrentInfo>> {
    let session = state.api.session();
    let save_path = session.output_folder().to_string_lossy().into_owned();
    let mut torrents: Vec<TorrentInfo> = session.with_torrents(|torrents| {
        torrents
            .filter_map(|(_, torrent)| {
                let (stats, automation, _) = torrent.refresh_automation_stats(unix_time_seconds());
                if !query.category.is_empty() && automation.category != query.category {
                    return None;
                }

                let metadata = torrent.metadata.load();
                let content_path = metadata.as_ref().map(|metadata| {
                    if metadata.file_infos.len() == 1 {
                        torrent
                            .output_folder()
                            .join(&metadata.file_infos[0].relative_filename)
                    } else {
                        torrent.output_folder().to_path_buf()
                    }
                });
                let progress = if stats.total_bytes == 0 {
                    f64::from(stats.finished)
                } else {
                    stats.progress_bytes as f64 / stats.total_bytes as f64
                };
                let ratio = share_ratio(automation.uploaded_bytes, stats.progress_bytes);
                let state_name = torrent_state_name(
                    stats.state,
                    stats.finished,
                    stats
                        .live
                        .as_ref()
                        .is_some_and(|live| live.download_speed.as_bytes() > 0),
                    stats
                        .live
                        .as_ref()
                        .is_some_and(|live| live.upload_speed.as_bytes() > 0),
                );
                let category = automation.category;

                Some(TorrentInfo {
                    hash: torrent.info_hash().as_string(),
                    name: torrent
                        .name()
                        .unwrap_or_else(|| torrent.info_hash().as_string()),
                    size: stats.total_bytes,
                    progress,
                    eta: UNKNOWN_ETA_SECONDS,
                    state: state_name,
                    label: category.clone(),
                    category,
                    save_path: save_path.clone(),
                    content_path: content_path
                        .unwrap_or_else(|| torrent.output_folder().to_path_buf())
                        .to_string_lossy()
                        .into_owned(),
                    ratio,
                    ratio_limit: automation.ratio_limit.unwrap_or(-2.0),
                    seeding_time: automation.seeding_seconds,
                    seeding_time_limit: automation
                        .seeding_time_limit_seconds
                        .and_then(|seconds| i64::try_from(seconds / 60).ok())
                        .unwrap_or(-2),
                    inactive_seeding_time_limit: -2,
                    last_activity: automation.last_activity_unix_seconds.unwrap_or_default(),
                })
            })
            .collect()
    });
    for pending in session.pending_automation_torrents() {
        if !query.category.is_empty() && pending.automation.category != query.category {
            continue;
        }
        let category = pending.automation.category;
        torrents.push(TorrentInfo {
            hash: pending.info_hash.as_string(),
            name: pending
                .name
                .unwrap_or_else(|| pending.info_hash.as_string()),
            size: 0,
            progress: 0.0,
            eta: UNKNOWN_ETA_SECONDS,
            state: match pending.state {
                PendingAutomationTorrentState::ResolvingMetadata => "metaDL",
                PendingAutomationTorrentState::Error => "error",
            },
            label: category.clone(),
            category,
            save_path: save_path.clone(),
            content_path: save_path.clone(),
            ratio: 0.0,
            ratio_limit: -2.0,
            seeding_time: 0,
            seeding_time_limit: -2,
            inactive_seeding_time_limit: -2,
            last_activity: 0,
        });
    }
    Json(torrents)
}

fn share_ratio(uploaded_bytes: u64, downloaded_bytes: u64) -> f64 {
    if downloaded_bytes == 0 {
        0.0
    } else {
        uploaded_bytes as f64 / downloaded_bytes as f64
    }
}

fn torrent_state_name(
    state: TorrentStatsState,
    finished: bool,
    downloading: bool,
    uploading: bool,
) -> &'static str {
    match state {
        TorrentStatsState::Initializing { .. } => "checkingDL",
        TorrentStatsState::Paused if finished => "pausedUP",
        TorrentStatsState::Paused => "pausedDL",
        TorrentStatsState::Live if finished && uploading => "uploading",
        TorrentStatsState::Live if finished => "stalledUP",
        TorrentStatsState::Live if downloading => "downloading",
        TorrentStatsState::Live => "stalledDL",
        TorrentStatsState::Error => "error",
    }
}

#[derive(Deserialize)]
struct HashQuery {
    hash: String,
}

fn parse_hash(hash: &str) -> QbitResult<TorrentIdOrHash> {
    TorrentIdOrHash::parse(hash)
        .with_context(|| format!("invalid torrent hash {hash:?}"))
        .map_err(|error| QbitError::bad_request(format!("{error:#}")))
}

async fn torrent_properties(
    State(state): State<ApiState>,
    Query(query): Query<HashQuery>,
) -> QbitResult<Json<TorrentProperties>> {
    let torrent = state
        .api
        .session()
        .get(parse_hash(&query.hash)?)
        .ok_or_else(|| QbitError::not_found("torrent not found"))?;
    let (stats, automation, _) = torrent.refresh_automation_stats(unix_time_seconds());
    let metadata = torrent
        .metadata
        .load_full()
        .ok_or_else(|| QbitError::conflict("torrent metadata is not available"))?;
    let share_ratio = share_ratio(automation.uploaded_bytes, stats.progress_bytes);

    Ok(Json(TorrentProperties {
        save_path: state
            .api
            .session()
            .output_folder()
            .to_string_lossy()
            .into_owned(),
        total_size: stats.total_bytes,
        total_downloaded: stats.progress_bytes,
        total_uploaded: automation.uploaded_bytes,
        total_wasted: 0,
        time_elapsed: 0,
        seeding_time: automation.seeding_seconds,
        nb_connections: stats
            .live
            .as_ref()
            .map(|live| live.snapshot.peer_stats.live as u64)
            .unwrap_or_default(),
        share_ratio,
        addition_date: automation.added_at_unix_seconds,
        completion_date: automation.completed_at_unix_seconds.unwrap_or_default(),
        created_by: String::new(),
        dl_limit: -1,
        up_limit: -1,
        piece_size: metadata.info.lengths().default_piece_length(),
    }))
}

async fn torrent_files(
    State(state): State<ApiState>,
    Query(query): Query<HashQuery>,
) -> QbitResult<Json<Vec<TorrentFile>>> {
    let torrent = state
        .api
        .session()
        .get(parse_hash(&query.hash)?)
        .ok_or_else(|| QbitError::not_found("torrent not found"))?;
    let stats = torrent.stats();
    let metadata = torrent
        .metadata
        .load_full()
        .ok_or_else(|| QbitError::conflict("torrent metadata is not available"))?;
    let only_files = torrent.only_files();
    let files = metadata
        .file_infos
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let downloaded = stats.file_progress.get(index).copied().unwrap_or_default();
            TorrentFile {
                index,
                name: file.relative_filename.to_string_lossy().into_owned(),
                size: file.len,
                progress: if file.len == 0 {
                    1.0
                } else {
                    downloaded as f64 / file.len as f64
                },
                priority: u8::from(
                    only_files
                        .as_ref()
                        .is_none_or(|selected| selected.contains(&index)),
                ),
                is_seed: downloaded == file.len,
            }
        })
        .collect();
    Ok(Json(files))
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddForm {
    urls: Option<String>,
    category: Option<String>,
    paused: Option<bool>,
    content_layout: Option<String>,
    sequential_download: Option<bool>,
    first_last_piece_prio: Option<bool>,
    ratio_limit: Option<f64>,
    seeding_time_limit: Option<i64>,
}

async fn add_torrent(State(state): State<ApiState>, request: Request) -> QbitResult<Response> {
    let content_type = request
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();

    let (torrent, form) = if content_type.starts_with("multipart/form-data") {
        parse_multipart(request, &state).await?
    } else if content_type.starts_with("application/x-www-form-urlencoded") {
        let bytes = to_bytes(request.into_body(), 1024 * 1024)
            .await
            .map_err(|_| QbitError::bad_request("invalid form body"))?;
        let form: AddForm = serde_urlencoded::from_bytes(&bytes)
            .map_err(|error| QbitError::bad_request(format!("invalid form: {error}")))?;
        let url = form
            .urls
            .clone()
            .ok_or_else(|| QbitError::bad_request("missing urls"))?;
        (add_torrent_from_url(url)?, form)
    } else {
        return Err(QbitError::bad_request("unsupported content type"));
    };

    validate_add_form(&form)?;
    let category = form.category.unwrap_or_default();
    if !state.api.session().has_automation_category(&category) {
        return Err(QbitError::conflict("category does not exist"));
    }

    let options = AddTorrentOptions {
        paused: form.paused.unwrap_or(false),
        overwrite: true,
        automation: TorrentAutomationMetadata {
            category,
            ratio_limit: parse_ratio_limit(form.ratio_limit)?,
            seeding_time_limit_seconds: parse_seeding_time_limit(form.seeding_time_limit)?,
            ..Default::default()
        },
        ..Default::default()
    };
    let torrent = match torrent {
        AddTorrent::Url(magnet_url) => {
            let added = state
                .api
                .session()
                .add_pending_automation_magnet(magnet_url.into_owned(), options)
                .map_err(|error| {
                    QbitError::internal(StatusCode::BAD_REQUEST, "failed to add magnet", error)
                })?;
            return Ok((StatusCode::OK, if added { "" } else { "Fails." }).into_response());
        }
        torrent => torrent,
    };

    let result = state
        .api
        .session()
        .add_torrent(torrent, Some(options))
        .await
        .map_err(|error| {
            QbitError::internal(StatusCode::BAD_REQUEST, "failed to add torrent", error)
        })?;

    Ok(match result {
        AddTorrentResponse::AlreadyManaged(_, _) => (StatusCode::OK, "Fails.").into_response(),
        AddTorrentResponse::Added(_, _) => (StatusCode::OK, "").into_response(),
        AddTorrentResponse::ListOnly(_) => {
            QbitError::bad_request("list-only add is unsupported").into_response()
        }
    })
}

fn add_torrent_from_url(url: String) -> QbitResult<AddTorrent<'static>> {
    if url.len() > MAX_MAGNET_URL_SIZE {
        return Err(QbitError::bad_request("magnet URL is too large"));
    }
    if !url.starts_with("magnet:") {
        return Err(QbitError::conflict(
            "only magnet URLs are supported by this compatibility profile",
        ));
    }
    Ok(AddTorrent::from_url(url))
}

async fn parse_multipart(
    request: Request,
    state: &ApiState,
) -> QbitResult<(AddTorrent<'static>, AddForm)> {
    let mut multipart = Multipart::from_request(request, state)
        .await
        .map_err(|error| QbitError::bad_request(format!("invalid multipart body: {error}")))?;
    let mut form = AddForm::default();
    let mut torrent_bytes = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|error| QbitError::bad_request(format!("invalid multipart field: {error}")))?
    {
        let name = field.name().unwrap_or_default().to_owned();
        if name == "torrents" {
            torrent_bytes =
                Some(field.bytes().await.map_err(|error| {
                    QbitError::bad_request(format!("invalid torrent: {error}"))
                })?);
            continue;
        }
        let value = field
            .text()
            .await
            .map_err(|error| QbitError::bad_request(format!("invalid field: {error}")))?;
        match name.as_str() {
            "urls" => form.urls = Some(value),
            "category" => form.category = Some(value),
            "paused" => form.paused = Some(parse_bool(&value)?),
            "contentLayout" => form.content_layout = Some(value),
            "sequentialDownload" => form.sequential_download = Some(parse_bool(&value)?),
            "firstLastPiecePrio" => form.first_last_piece_prio = Some(parse_bool(&value)?),
            "ratioLimit" => {
                form.ratio_limit = Some(
                    value
                        .parse()
                        .map_err(|_| QbitError::bad_request("invalid ratioLimit"))?,
                )
            }
            "seedingTimeLimit" => {
                form.seeding_time_limit = Some(
                    value
                        .parse()
                        .map_err(|_| QbitError::bad_request("invalid seedingTimeLimit"))?,
                )
            }
            _ => {}
        }
    }

    let torrent = match torrent_bytes {
        Some(bytes) => AddTorrent::from_bytes(bytes),
        None => {
            let url = form
                .urls
                .clone()
                .ok_or_else(|| QbitError::bad_request("missing torrents upload or urls"))?;
            add_torrent_from_url(url)?
        }
    };
    Ok((torrent, form))
}

fn parse_bool(value: &str) -> QbitResult<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(QbitError::bad_request("invalid boolean")),
    }
}

fn validate_add_form(form: &AddForm) -> QbitResult<()> {
    if form.sequential_download == Some(true) {
        return Err(QbitError::conflict("sequential download is unsupported"));
    }
    if form.first_last_piece_prio == Some(true) {
        return Err(QbitError::conflict(
            "first/last piece priority is unsupported",
        ));
    }
    if form
        .content_layout
        .as_deref()
        .is_some_and(|layout| layout != "Original")
    {
        return Err(QbitError::conflict("unsupported content layout"));
    }
    Ok(())
}

fn parse_ratio_limit(value: Option<f64>) -> QbitResult<Option<f64>> {
    match value {
        None | Some(-2.0) | Some(-1.0) => Ok(None),
        Some(value) if value.is_finite() && value >= 0.0 => Ok(Some(value)),
        Some(_) => Err(QbitError::bad_request("invalid ratioLimit")),
    }
}

fn parse_seeding_time_limit(value: Option<i64>) -> QbitResult<Option<u64>> {
    match value {
        None | Some(-2) | Some(-1) => Ok(None),
        Some(value) if value >= 0 => (value as u64)
            .checked_mul(60)
            .map(Some)
            .ok_or_else(|| QbitError::bad_request("seedingTimeLimit is too large")),
        Some(_) => Err(QbitError::bad_request("invalid seedingTimeLimit")),
    }
}

#[derive(Deserialize)]
struct CategoryForm {
    category: String,
}

async fn categories(State(state): State<ApiState>) -> Json<BTreeMap<String, Category>> {
    Json(
        state
            .api
            .session()
            .automation_categories()
            .into_iter()
            .map(|category| {
                let name = category.name;
                (
                    name.clone(),
                    Category {
                        name,
                        save_path: String::new(),
                    },
                )
            })
            .collect(),
    )
}

async fn create_category(
    State(state): State<ApiState>,
    Form(form): Form<CategoryForm>,
) -> QbitResult<StatusCode> {
    validate_category(&form.category)?;
    if !state
        .api
        .session()
        .create_automation_category(form.category)
        .await
        .map_err(|error| {
            QbitError::internal(StatusCode::CONFLICT, "failed to store category", error)
        })?
    {
        return Err(QbitError::conflict("category already exists"));
    }
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct RemoveCategoriesForm {
    categories: String,
}

async fn remove_categories(
    State(state): State<ApiState>,
    Form(form): Form<RemoveCategoriesForm>,
) -> QbitResult<StatusCode> {
    for category in form.categories.lines() {
        state
            .api
            .session()
            .delete_automation_category(category.trim_end_matches('\r'))
            .await
            .map_err(|error| {
                QbitError::internal(StatusCode::CONFLICT, "failed to delete category", error)
            })?;
    }
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct SetCategoryForm {
    hashes: String,
    category: String,
}

async fn set_category(
    State(state): State<ApiState>,
    Form(form): Form<SetCategoryForm>,
) -> QbitResult<StatusCode> {
    if !form.category.is_empty() {
        validate_category(&form.category)?;
    }
    for hash in parse_hashes(&form.hashes)? {
        if let TorrentIdOrHash::Hash(info_hash) = hash
            && state.api.session().get(hash).is_none()
            && state
                .api
                .session()
                .set_pending_automation_category(info_hash, form.category.clone())
        {
            continue;
        }
        state
            .api
            .session()
            .set_torrent_automation_category(hash, form.category.clone())
            .await
            .map_err(|error| {
                QbitError::internal(StatusCode::CONFLICT, "failed to set category", error)
            })?;
    }
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShareLimitsForm {
    hashes: String,
    ratio_limit: Option<f64>,
    seeding_time_limit: Option<i64>,
    inactive_seeding_time_limit: Option<i64>,
}

async fn set_share_limits(
    State(state): State<ApiState>,
    Form(form): Form<ShareLimitsForm>,
) -> QbitResult<StatusCode> {
    if form
        .inactive_seeding_time_limit
        .is_some_and(|limit| limit >= 0)
    {
        return Err(QbitError::conflict(
            "inactive seeding time limits are unsupported",
        ));
    }
    let ratio_limit = parse_ratio_limit(form.ratio_limit)?;
    let seeding_time_limit_seconds = parse_seeding_time_limit(form.seeding_time_limit)?;
    for hash in parse_hashes(&form.hashes)? {
        state
            .api
            .session()
            .set_torrent_automation_limits(hash, ratio_limit, seeding_time_limit_seconds)
            .await
            .map_err(|error| {
                QbitError::internal(StatusCode::CONFLICT, "failed to set share limits", error)
            })?;
    }
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
struct TorrentActionForm {
    hashes: String,
}

async fn pause_torrents(
    State(state): State<ApiState>,
    Form(form): Form<TorrentActionForm>,
) -> QbitResult<StatusCode> {
    for hash in parse_hashes(&form.hashes)? {
        let torrent = state
            .api
            .session()
            .get(hash)
            .ok_or_else(|| QbitError::not_found("torrent not found"))?;
        if !torrent.is_paused() {
            state
                .api
                .api_torrent_action_pause(hash)
                .await
                .map_err(|error| {
                    QbitError::internal(StatusCode::CONFLICT, "failed to pause torrent", error)
                })?;
        }
    }
    Ok(StatusCode::OK)
}

async fn resume_torrents(
    State(state): State<ApiState>,
    Form(form): Form<TorrentActionForm>,
) -> QbitResult<StatusCode> {
    for hash in parse_hashes(&form.hashes)? {
        let torrent = state
            .api
            .session()
            .get(hash)
            .ok_or_else(|| QbitError::not_found("torrent not found"))?;
        if torrent.is_paused() {
            state
                .api
                .api_torrent_action_start(hash)
                .await
                .map_err(|error| {
                    QbitError::internal(StatusCode::CONFLICT, "failed to resume torrent", error)
                })?;
        }
    }
    Ok(StatusCode::OK)
}

async fn top_priority(Form(form): Form<TorrentActionForm>) -> QbitResult<StatusCode> {
    parse_hashes(&form.hashes)?;
    Err(QbitError::conflict("torrent queueing is disabled"))
}

#[derive(Deserialize)]
struct ForceStartForm {
    hashes: String,
    value: bool,
}

async fn set_force_start(
    State(state): State<ApiState>,
    Form(form): Form<ForceStartForm>,
) -> QbitResult<StatusCode> {
    if form.value {
        return resume_torrents(
            State(state),
            Form(TorrentActionForm {
                hashes: form.hashes,
            }),
        )
        .await;
    }
    for hash in parse_hashes(&form.hashes)? {
        if state.api.session().get(hash).is_none() {
            return Err(QbitError::not_found("torrent not found"));
        }
    }
    Ok(StatusCode::OK)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeleteForm {
    hashes: String,
    delete_files: Option<bool>,
}

async fn delete_torrents(
    State(state): State<ApiState>,
    Form(form): Form<DeleteForm>,
) -> QbitResult<StatusCode> {
    for hash in parse_hashes(&form.hashes)? {
        if let TorrentIdOrHash::Hash(info_hash) = hash
            && state.api.session().get(hash).is_none()
            && state
                .api
                .session()
                .forget_pending_automation_torrent(info_hash)
        {
            continue;
        }
        if state.api.session().get(hash).is_none() {
            return Err(QbitError::not_found("torrent not found"));
        }
        let result = if form.delete_files == Some(true) {
            state.api.api_torrent_action_delete(hash).await
        } else {
            state.api.api_torrent_action_forget(hash).await
        };
        result.map_err(|error| {
            QbitError::internal(StatusCode::CONFLICT, "failed to delete torrent", error)
        })?;
    }
    Ok(StatusCode::OK)
}

fn parse_hashes(hashes: &str) -> QbitResult<Vec<TorrentIdOrHash>> {
    let hashes = hashes
        .split('|')
        // Stop parsing once the request is known to exceed the supported batch size.
        .take(129)
        .map(|hash| {
            TorrentIdOrHash::parse(hash)
                .with_context(|| format!("invalid torrent hash {hash:?}"))
                .map_err(|error| QbitError::bad_request(format!("{error:#}")))
        })
        .collect::<QbitResult<Vec<_>>>()?;
    if hashes.is_empty() || hashes.len() > 128 {
        return Err(QbitError::bad_request("invalid number of torrent hashes"));
    }
    Ok(hashes)
}

fn validate_category(category: &str) -> QbitResult<()> {
    if category.is_empty()
        || category.len() > 128
        || category.starts_with('/')
        || category.ends_with('/')
        || category.contains("//")
        || category.contains('\\')
        || category.chars().any(char::is_control)
    {
        return Err(QbitError::bad_request("invalid category"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        net::{Ipv4Addr, SocketAddr},
        path::Path,
        sync::Arc,
        time::{Duration, Instant},
    };

    use axum::{Router, routing::get};
    use serde_json::Value;

    use super::{
        MAX_LOGIN_BODY_SIZE, QbitError, make_api_router, parse_hash, share_ratio,
        torrent_state_name, unix_time_seconds,
    };
    use crate::{
        AddTorrent, AddTorrentOptions, Api, CreateTorrentOptions, Session, SessionOptions,
        SessionPersistenceConfig, TorrentAutomationMetadata, TorrentStatsState,
        api::TorrentIdOrHash,
        create_torrent,
        http_api::{HttpApi, HttpApiOptions},
        spawn_utils::BlockingSpawner,
    };

    async fn start_server(router: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (format!("http://{address}"), task)
    }

    async fn start_empty_tracker() -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route("/announce", get(|| async { "d8:intervali60e5:peers0:e" })),
            )
            .await
            .unwrap();
        });
        (format!("http://{address}/announce"), task)
    }

    async fn start_full_http_api(
        session: Arc<Session>,
        enabled: bool,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = librqbit_dualstack_sockets::TcpListener::bind_tcp(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            Default::default(),
        )
        .unwrap();
        let address = listener.bind_addr();
        let server = HttpApi::new(
            Api::new(session.clone(), None, None),
            Some(HttpApiOptions {
                enable_qbittorrent_api: enabled,
                basic_auth: enabled.then(|| ("test".to_owned(), "secret".to_owned())),
                ..Default::default()
            }),
        )
        .make_http_api_and_run(listener, None);
        let task = tokio::spawn(async move { server.await.unwrap() });
        (format!("http://{address}/api/v2"), task)
    }

    fn multipart_body(boundary: &str, category: &str, filename: &str, torrent: &[u8]) -> Vec<u8> {
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"category\"\r\n\r\n{category}\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"paused\"\r\n\r\ntrue\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"torrents\"; filename=\"{filename}\"\r\n\
             Content-Type: application/x-bittorrent\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(torrent);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        body
    }

    fn multipart_url_body(boundary: &str, category: &str, url: &str) -> String {
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"urls\"\r\n\r\n{url}\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"category\"\r\n\r\n{category}\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"paused\"\r\n\r\nfalse\r\n\
             --{boundary}\r\nContent-Disposition: form-data; name=\"contentLayout\"\r\n\r\nOriginal\r\n\
             --{boundary}--\r\n"
        )
    }

    async fn make_fixture(path: &Path) -> Vec<u8> {
        create_torrent(
            path,
            CreateTorrentOptions::default(),
            &BlockingSpawner::new(1),
        )
        .await
        .unwrap()
        .as_bytes()
        .unwrap()
        .to_vec()
    }

    async fn add_fixture(
        client: &reqwest::Client,
        base: &str,
        cookie: &str,
        filename: &str,
        torrent: &[u8],
    ) {
        let boundary = "rqbit-servarr-contract-boundary";
        let response = client
            .post(format!("{base}/torrents/add"))
            .header("Cookie", cookie)
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(multipart_body(boundary, "sonarr", filename, torrent))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), "");
    }

    #[tokio::test]
    async fn first_milestone_http_contract() {
        let downloads = tempfile::tempdir().unwrap();
        let fixtures = tempfile::tempdir().unwrap();
        let single_name = "Movie ünicode.mkv";
        let single_source = fixtures.path().join(single_name);
        std::fs::write(&single_source, b"legal single-file fixture").unwrap();
        std::fs::copy(&single_source, downloads.path().join(single_name)).unwrap();
        let single_torrent = make_fixture(&single_source).await;

        let multi_source = fixtures.path().join("Show Name");
        std::fs::create_dir(&multi_source).unwrap();
        std::fs::create_dir(multi_source.join("Season 01")).unwrap();
        std::fs::write(
            multi_source.join("Season 01").join("Episode 01.mkv"),
            b"legal multi-file fixture one",
        )
        .unwrap();
        std::fs::write(
            multi_source.join("Season 01").join("Episode 02.mkv"),
            b"legal multi-file fixture two",
        )
        .unwrap();
        copy_dir_all(&multi_source, &downloads.path().join("Show Name"));
        let multi_torrent = make_fixture(&multi_source).await;

        let session = Session::new_with_opts(
            downloads.path().to_path_buf(),
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let state = Arc::new(HttpApi::new(
            Api::new(session, None, None),
            Some(HttpApiOptions {
                read_only: false,
                basic_auth: Some(("servarr".to_owned(), "secret".to_owned())),
                enable_qbittorrent_api: true,
                ..Default::default()
            }),
        ));
        let (base, server) = start_server(make_api_router(state)).await;
        let client = reqwest::Client::new();

        let response = client
            .post(format!("{base}/auth/login"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!(
                "username={}&password=wrong",
                "a".repeat(MAX_LOGIN_BODY_SIZE)
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);

        let response = client
            .get(format!("{base}/app/webapiVersion"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);

        let response = client
            .get(format!("{base}/app/webapiVersion"))
            .basic_auth("servarr", Some("wrong"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);

        let response = client
            .get(format!("{base}/app/webapiVersion"))
            .basic_auth("servarr", Some("secret"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let response = client
            .post(format!("{base}/auth/login"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("username=servarr&password=wrong")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), "Fails.");

        let response = client
            .post(format!("{base}/auth/login"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("username=servarr&password=secret")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let cookie = response
            .headers()
            .get("Set-Cookie")
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        assert_eq!(response.text().await.unwrap(), "Ok.");

        let response = client
            .get(format!("{base}/app/webapiVersion"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(response.text().await.unwrap(), "2.8.1");

        let response = client
            .get(format!("{base}/app/version"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.text().await.unwrap(),
            format!("v{}", crate::version())
        );

        let preferences: Value = client
            .get(format!("{base}/app/preferences"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            preferences["save_path"],
            downloads.path().to_string_lossy().as_ref()
        );
        assert_eq!(preferences["queueing_enabled"], false);

        let response = client
            .post(format!("{base}/torrents/createCategory"))
            .header("Cookie", &cookie)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("category=sonarr")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let categories: Value = client
            .get(format!("{base}/torrents/categories"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(categories["sonarr"]["name"], "sonarr");
        assert_eq!(categories["sonarr"]["savePath"], "");

        add_fixture(&client, &base, &cookie, "single.torrent", &single_torrent).await;
        add_fixture(&client, &base, &cookie, "multi.torrent", &multi_torrent).await;

        let torrents: Value = client
            .get(format!("{base}/torrents/info?category=sonarr"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let torrents = torrents.as_array().unwrap();
        assert_eq!(torrents.len(), 2);
        for torrent in torrents {
            assert_eq!(
                torrent["save_path"],
                downloads.path().to_string_lossy().as_ref()
            );
            let expected_content_path = if torrent["name"] == single_name {
                downloads.path().join(single_name)
            } else {
                downloads.path().join("Show Name")
            };
            assert_eq!(
                torrent["content_path"],
                expected_content_path.to_string_lossy().as_ref()
            );
            assert_ne!(torrent["content_path"], torrent["save_path"]);
        }

        let inspected_hash = torrents[0]["hash"].as_str().unwrap();
        let properties: Value = client
            .get(format!("{base}/torrents/properties?hash={inspected_hash}"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            properties["save_path"],
            downloads.path().to_string_lossy().as_ref()
        );
        assert!(properties["total_size"].as_u64().unwrap() > 0);

        let files: Value = client
            .get(format!("{base}/torrents/files?hash={inspected_hash}"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(!files.as_array().unwrap().is_empty());
        assert!(files[0]["name"].as_str().is_some());

        let first_hash = torrents[0]["hash"].as_str().unwrap();
        let kept_path = if torrents[0]["name"] == single_name {
            downloads.path().join(single_name)
        } else {
            downloads
                .path()
                .join("Show Name")
                .join("Season 01")
                .join("Episode 01.mkv")
        };
        let bytes_before = std::fs::read(&kept_path).unwrap();
        let response = client
            .post(format!("{base}/torrents/delete"))
            .header("Cookie", &cookie)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!("hashes={first_hash}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(std::fs::read(&kept_path).unwrap(), bytes_before);

        let remaining: Value = client
            .get(format!("{base}/torrents/info?category=sonarr"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(remaining.as_array().unwrap().len(), 1);

        let response = client
            .post(format!("{base}/torrents/removeCategories"))
            .header("Cookie", &cookie)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("categories=sonarr")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        let categories: Value = client
            .get(format!("{base}/torrents/categories"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(categories.get("sonarr").is_none());

        let remaining: Value = client
            .get(format!("{base}/torrents/info"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(remaining.as_array().unwrap().len(), 1);
        assert_eq!(remaining[0]["category"], "");

        let remaining_hash = remaining[0]["hash"].as_str().unwrap();
        let kept_path = if remaining[0]["name"] == single_name {
            downloads.path().join(single_name)
        } else {
            downloads
                .path()
                .join("Show Name")
                .join("Season 01")
                .join("Episode 02.mkv")
        };
        let bytes_before = std::fs::read(&kept_path).unwrap();
        let response = client
            .post(format!("{base}/torrents/delete"))
            .header("Cookie", &cookie)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!("hashes={remaining_hash}&deleteFiles=false"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(std::fs::read(&kept_path).unwrap(), bytes_before);

        let remaining: Value = client
            .get(format!("{base}/torrents/info?category=sonarr"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(remaining.as_array().unwrap().is_empty());

        let response = client
            .post(format!("{base}/auth/logout"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let response = client
            .get(format!("{base}/app/version"))
            .header("Cookie", &cookie)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);

        server.abort();
    }

    #[tokio::test]
    async fn compatibility_routes_are_opt_in() {
        let downloads = tempfile::tempdir().unwrap();
        let make_session = || {
            Session::new_with_opts(
                downloads.path().to_path_buf(),
                SessionOptions {
                    dht: None,
                    listen: None,
                    disable_local_service_discovery: true,
                    ..Default::default()
                },
            )
        };
        let client = reqwest::Client::new();

        let (base, disabled_server) =
            start_full_http_api(make_session().await.unwrap(), false).await;
        let response = client
            .get(format!("{base}/app/webapiVersion"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
        disabled_server.abort();

        let (base, enabled_server) = start_full_http_api(make_session().await.unwrap(), true).await;
        let response = client
            .get(format!("{base}/app/webapiVersion"))
            .basic_auth("test", Some("secret"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), "2.8.1");
        enabled_server.abort();
    }

    #[tokio::test]
    async fn read_only_compatibility_api_does_not_expose_mutations() {
        let downloads = tempfile::tempdir().unwrap();
        let session = Session::new_with_opts(
            downloads.path().to_path_buf(),
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let state = Arc::new(HttpApi::new(
            Api::new(session, None, None),
            Some(HttpApiOptions {
                read_only: true,
                basic_auth: Some(("test".to_owned(), "secret".to_owned())),
                enable_qbittorrent_api: true,
                ..Default::default()
            }),
        ));
        let (base, server) = start_server(make_api_router(state)).await;

        let response = reqwest::Client::new()
            .post(format!("{base}/torrents/createCategory"))
            .basic_auth("test", Some("secret"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body("category=forbidden")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
        server.abort();
    }

    #[test]
    fn internal_errors_do_not_expose_details() {
        let error = QbitError::internal(
            http::StatusCode::CONFLICT,
            "operation failed",
            anyhow::anyhow!("C:\\secret\\path"),
        );
        assert_eq!(error.message, "operation failed");
    }

    #[tokio::test]
    async fn compatibility_api_requires_non_empty_credentials() {
        let downloads = tempfile::tempdir().unwrap();
        let session = Session::new_with_opts(
            downloads.path().to_path_buf(),
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let listener = librqbit_dualstack_sockets::TcpListener::bind_tcp(
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            Default::default(),
        )
        .unwrap();

        let result = HttpApi::new(
            Api::new(session, None, None),
            Some(HttpApiOptions {
                enable_qbittorrent_api: true,
                ..Default::default()
            }),
        )
        .make_http_api_and_run(listener, None)
        .await;

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("requires a non-empty username and password")
        );
    }

    #[tokio::test]
    async fn writable_non_loopback_api_requires_credentials() {
        let downloads = tempfile::tempdir().unwrap();
        let session = Session::new_with_opts(
            downloads.path().to_path_buf(),
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let listener = librqbit_dualstack_sockets::TcpListener::bind_tcp(
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0)),
            Default::default(),
        )
        .unwrap();

        let result = HttpApi::new(
            Api::new(session, None, None),
            Some(HttpApiOptions {
                read_only: false,
                ..Default::default()
            }),
        )
        .make_http_api_and_run(listener, None)
        .await;

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("writable HTTP API on a non-loopback address requires")
        );
    }

    #[test]
    fn state_and_ratio_mapping_contract() {
        assert_eq!(
            torrent_state_name(
                TorrentStatsState::Initializing { paused: false },
                false,
                false,
                false
            ),
            "checkingDL"
        );
        assert_eq!(
            torrent_state_name(TorrentStatsState::Paused, false, false, false),
            "pausedDL"
        );
        assert_eq!(
            torrent_state_name(TorrentStatsState::Paused, true, false, false),
            "pausedUP"
        );
        assert_eq!(
            torrent_state_name(TorrentStatsState::Live, false, true, false),
            "downloading"
        );
        assert_eq!(
            torrent_state_name(TorrentStatsState::Live, false, false, false),
            "stalledDL"
        );
        assert_eq!(
            torrent_state_name(TorrentStatsState::Live, true, false, true),
            "uploading"
        );
        assert_eq!(
            torrent_state_name(TorrentStatsState::Live, true, false, false),
            "stalledUP"
        );
        assert_eq!(
            torrent_state_name(TorrentStatsState::Error, false, false, false),
            "error"
        );
        assert_eq!(share_ratio(10, 0), 0.0);
        assert_eq!(share_ratio(50, 100), 0.5);
        assert_eq!(share_ratio(200, 100), 2.0);
    }

    #[tokio::test]
    async fn category_and_torrent_category_survive_json_restart() {
        let downloads = tempfile::tempdir().unwrap();
        let persistence = tempfile::tempdir().unwrap();
        let fixture = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(fixture.path(), b"legal persistence fixture").unwrap();
        let managed_path = downloads.path().join(fixture.path().file_name().unwrap());
        std::fs::copy(fixture.path(), &managed_path).unwrap();
        let torrent_bytes = make_fixture(fixture.path()).await;

        let make_session = || {
            Session::new_with_opts(
                downloads.path().to_path_buf(),
                SessionOptions {
                    dht: None,
                    listen: None,
                    disable_local_service_discovery: true,
                    persistence: Some(SessionPersistenceConfig::Json {
                        folder: Some(persistence.path().to_path_buf()),
                    }),
                    ..Default::default()
                },
            )
        };

        let session = make_session().await.unwrap();
        assert!(
            session
                .create_automation_category("sonarr".to_owned())
                .await
                .unwrap()
        );
        session
            .add_torrent(
                AddTorrent::from_bytes(torrent_bytes),
                Some(AddTorrentOptions {
                    paused: true,
                    overwrite: true,
                    automation: TorrentAutomationMetadata {
                        category: "sonarr".to_owned(),
                        ratio_limit: Some(1.5),
                        seeding_time_limit_seconds: Some(600),
                        uploaded_bytes: 200,
                        seeding_seconds: 42,
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            )
            .await
            .unwrap();
        session.stop().await;
        drop(session);

        let restored = make_session().await.unwrap();
        assert!(restored.has_automation_category("sonarr"));
        let metadata = restored.with_torrents(|torrents| {
            torrents
                .map(|(_, torrent)| torrent.automation_metadata())
                .collect::<Vec<_>>()
        });
        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].category, "sonarr");
        assert_eq!(metadata[0].ratio_limit, Some(1.5));
        assert_eq!(metadata[0].seeding_time_limit_seconds, Some(600));
        assert_eq!(metadata[0].uploaded_bytes, 200);
        assert_eq!(metadata[0].seeding_seconds, 42);

        let state = Arc::new(HttpApi::new(
            Api::new(restored.clone(), None, None),
            Some(HttpApiOptions {
                enable_qbittorrent_api: true,
                ..Default::default()
            }),
        ));
        let (base, server) = start_server(make_api_router(state)).await;
        let torrents: Value = reqwest::get(format!("{base}/torrents/info?category=sonarr"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            torrents[0]["content_path"],
            managed_path.to_string_lossy().as_ref()
        );
        server.abort();

        assert!(restored.delete_automation_category("sonarr").await.unwrap());
        assert!(!restored.has_automation_category("sonarr"));
        assert_eq!(
            restored.with_torrents(|torrents| {
                torrents.next().unwrap().1.automation_metadata().category
            }),
            ""
        );
        restored.stop().await;
        drop(restored);

        let after_category_delete = make_session().await.unwrap();
        assert!(!after_category_delete.has_automation_category("sonarr"));
        assert_eq!(
            after_category_delete.with_torrents(|torrents| {
                torrents.next().unwrap().1.automation_metadata().category
            }),
            ""
        );

        let info_hash =
            after_category_delete.with_torrents(|torrents| torrents.next().unwrap().1.info_hash());
        after_category_delete
            .delete(TorrentIdOrHash::Hash(info_hash), false)
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(&managed_path).unwrap(),
            b"legal persistence fixture"
        );
        after_category_delete.stop().await;
        drop(after_category_delete);

        let after_delete = make_session().await.unwrap();
        assert_eq!(after_delete.with_torrents(|torrents| torrents.count()), 0);
        after_delete.stop().await;
    }

    #[tokio::test]
    async fn share_limit_is_enforced_after_torrent_completes() {
        let downloads = tempfile::tempdir().unwrap();
        let fixtures = tempfile::tempdir().unwrap();
        let source = fixtures.path().join("seed-limit-fixture.mkv");
        std::fs::write(&source, b"legal seed limit fixture").unwrap();
        std::fs::copy(&source, downloads.path().join("seed-limit-fixture.mkv")).unwrap();
        let torrent_bytes = make_fixture(&source).await;
        let session = Session::new_with_opts(
            downloads.path().to_path_buf(),
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let torrent = session
            .add_torrent(
                AddTorrent::from_bytes(torrent_bytes),
                Some(AddTorrentOptions {
                    overwrite: true,
                    automation: TorrentAutomationMetadata {
                        ratio_limit: Some(0.0),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            )
            .await
            .unwrap()
            .into_handle()
            .unwrap();

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if torrent.stats().finished && torrent.is_paused() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("ratio limit was not enforced");
        assert_eq!(torrent.automation_metadata().ratio_limit, Some(0.0));
        session.stop().await;
    }

    #[tokio::test]
    async fn share_limits_and_pause_resume_http_controls() {
        let downloads = tempfile::tempdir().unwrap();
        let fixtures = tempfile::tempdir().unwrap();
        let source = fixtures.path().join("controls-fixture.mkv");
        std::fs::write(&source, b"legal controls fixture").unwrap();
        std::fs::copy(&source, downloads.path().join("controls-fixture.mkv")).unwrap();
        let torrent_bytes = make_fixture(&source).await;
        let session = Session::new_with_opts(
            downloads.path().to_path_buf(),
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        session
            .create_automation_category("sonarr".to_owned())
            .await
            .unwrap();
        let state = Arc::new(HttpApi::new(
            Api::new(session.clone(), None, None),
            Some(HttpApiOptions {
                enable_qbittorrent_api: true,
                ..Default::default()
            }),
        ));
        let (base, server) = start_server(make_api_router(state)).await;
        let client = reqwest::Client::new();
        add_fixture(&client, &base, "", "controls.torrent", &torrent_bytes).await;
        let listed: Value = client
            .get(format!("{base}/torrents/info?category=sonarr"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let hash = listed[0]["hash"].as_str().unwrap();
        let torrent = session.get(parse_hash(hash).unwrap()).unwrap();

        tokio::time::timeout(Duration::from_secs(5), async {
            while !matches!(torrent.stats().state, TorrentStatsState::Paused) {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap();

        let response = client
            .post(format!("{base}/torrents/setShareLimits"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!(
                "hashes={hash}&ratioLimit=2&seedingTimeLimit=30&inactiveSeedingTimeLimit=-2"
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        let listed: Value = client
            .get(format!("{base}/torrents/info?category=sonarr"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(listed[0]["ratio_limit"], 2.0);
        assert_eq!(listed[0]["seeding_time_limit"], 30);
        assert_eq!(listed[0]["state"], "pausedUP");

        let response = client
            .post(format!("{base}/torrents/resume"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!("hashes={hash}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert!(!torrent.is_paused());
        torrent.shared().automation_runtime.lock().last_seed_tick =
            Instant::now() - Duration::from_secs(2);
        let (_, automation, _) = torrent.refresh_automation_stats(unix_time_seconds());
        assert!(automation.seeding_seconds >= 2);

        let response = client
            .post(format!("{base}/torrents/pause"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!("hashes={hash}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert!(torrent.is_paused());
        let seeding_seconds = torrent.automation_metadata().seeding_seconds;
        torrent.shared().automation_runtime.lock().last_seed_tick =
            Instant::now() - Duration::from_secs(2);
        let (_, automation, _) = torrent.refresh_automation_stats(unix_time_seconds());
        assert_eq!(automation.seeding_seconds, seeding_seconds);

        session.stop().await;
        server.abort();
    }

    #[tokio::test]
    async fn magnet_add_returns_before_metadata_and_exposes_metadl() {
        let downloads = tempfile::tempdir().unwrap();
        let (tracker_url, tracker) = start_empty_tracker().await;
        let session = Session::new_with_opts(
            downloads.path().to_path_buf(),
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        session
            .create_automation_category("sonarr".to_owned())
            .await
            .unwrap();
        let state = Arc::new(HttpApi::new(
            Api::new(session.clone(), None, None),
            Some(HttpApiOptions {
                enable_qbittorrent_api: true,
                ..Default::default()
            }),
        ));
        let (base, server) = start_server(make_api_router(state)).await;
        let client = reqwest::Client::new();
        let info_hash = "0101010101010101010101010101010101010101";
        let magnet =
            format!("magnet:?xt=urn:btih:{info_hash}&dn=Pending%20Fixture&tr={tracker_url}");
        let add_body =
            serde_urlencoded::to_string([("urls", magnet.as_str()), ("category", "sonarr")])
                .unwrap();

        let response = tokio::time::timeout(
            Duration::from_millis(500),
            client
                .post(format!("{base}/torrents/add"))
                .header("Content-Type", "application/x-www-form-urlencoded")
                .body(add_body.clone())
                .send(),
        )
        .await
        .expect("magnet add blocked on metadata")
        .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), "");

        let torrents: Value = client
            .get(format!("{base}/torrents/info?category=sonarr"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(torrents[0]["hash"], info_hash);
        assert_eq!(torrents[0]["name"], "Pending Fixture");
        assert_eq!(torrents[0]["state"], "metaDL");

        let duplicate = client
            .post(format!("{base}/torrents/add"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(add_body)
            .send()
            .await
            .unwrap();
        assert_eq!(duplicate.text().await.unwrap(), "Fails.");

        let deleted = client
            .post(format!("{base}/torrents/delete"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!("hashes={info_hash}"))
            .send()
            .await
            .unwrap();
        assert_eq!(deleted.status(), reqwest::StatusCode::OK);

        server.abort();
        tracker.abort();
    }

    #[tokio::test]
    async fn multipart_magnet_add_matches_qbittorrent_contract() {
        let downloads = tempfile::tempdir().unwrap();
        let (tracker_url, tracker) = start_empty_tracker().await;
        let session = Session::new_with_opts(
            downloads.path().to_path_buf(),
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        session
            .create_automation_category("radarr".to_owned())
            .await
            .unwrap();
        let state = Arc::new(HttpApi::new(
            Api::new(session.clone(), None, None),
            Some(HttpApiOptions {
                enable_qbittorrent_api: true,
                ..Default::default()
            }),
        ));
        let (base, server) = start_server(make_api_router(state)).await;
        let info_hash = "0202020202020202020202020202020202020202";
        let magnet =
            format!("magnet:?xt=urn:btih:{info_hash}&dn=Multipart%20Fixture&tr={tracker_url}");
        let boundary = "rqbit-radarr-magnet-boundary";
        let client = reqwest::Client::new();

        let response = client
            .post(format!("{base}/torrents/add"))
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(multipart_url_body(boundary, "radarr", &magnet))
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), "");
        let torrents: Value = client
            .get(format!("{base}/torrents/info?category=radarr"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(torrents[0]["hash"], info_hash);
        assert_eq!(torrents[0]["name"], "Multipart Fixture");
        assert_eq!(torrents[0]["state"], "metaDL");

        server.abort();
        tracker.abort();
    }

    #[tokio::test]
    async fn delete_files_unlinks_managed_content_but_keeps_hardlink() {
        let downloads = tempfile::tempdir().unwrap();
        let imports = tempfile::tempdir().unwrap();
        let fixtures = tempfile::tempdir().unwrap();
        let filename = "hardlink-fixture.mkv";
        let source = fixtures.path().join(filename);
        let managed = downloads.path().join(filename);
        let imported = imports.path().join(filename);
        let expected = b"legal hardlink fixture";
        std::fs::write(&source, expected).unwrap();
        std::fs::copy(&source, &managed).unwrap();
        let torrent_bytes = make_fixture(&source).await;

        let session = Session::new_with_opts(
            downloads.path().to_path_buf(),
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        session
            .create_automation_category("sonarr".to_owned())
            .await
            .unwrap();
        let state = Arc::new(HttpApi::new(
            Api::new(session.clone(), None, None),
            Some(HttpApiOptions {
                enable_qbittorrent_api: true,
                ..Default::default()
            }),
        ));
        let (base, server) = start_server(make_api_router(state)).await;
        let client = reqwest::Client::new();
        add_fixture(&client, &base, "", filename, &torrent_bytes).await;

        let torrents: Value = client
            .get(format!("{base}/torrents/info?category=sonarr"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(torrents.as_array().unwrap().len(), 1);
        let hash = torrents[0]["hash"].as_str().unwrap();
        let torrent = session.get(parse_hash(hash).unwrap()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !matches!(torrent.stats().state, TorrentStatsState::Paused) {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap();
        std::fs::hard_link(&managed, &imported).unwrap();

        let response = client
            .post(format!("{base}/torrents/resume"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!("hashes={hash}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert!(matches!(torrent.stats().state, TorrentStatsState::Live));
        assert_eq!(std::fs::read(&imported).unwrap(), expected);

        let response = client
            .post(format!("{base}/torrents/delete"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!("hashes={hash}&deleteFiles=true"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert!(!managed.exists());
        assert_eq!(std::fs::read(imported).unwrap(), expected);

        std::fs::copy(&source, &managed).unwrap();
        add_fixture(&client, &base, "", filename, &torrent_bytes).await;
        std::fs::remove_file(&managed).unwrap();
        let response = client
            .post(format!("{base}/torrents/delete"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(format!("hashes={hash}&deleteFiles=true"))
            .send()
            .await
            .unwrap();
        assert!(!response.status().is_success());
        let still_managed: Value = client
            .get(format!("{base}/torrents/info?category=sonarr"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(still_managed.as_array().unwrap().len(), 1);

        server.abort();
    }

    #[tokio::test]
    async fn directory_delete_preserves_unrelated_file_and_survives_restart() {
        let downloads = tempfile::tempdir().unwrap();
        let persistence = tempfile::tempdir().unwrap();
        let fixtures = tempfile::tempdir().unwrap();
        let source_root = fixtures.path().join("Directory Torrent");
        std::fs::create_dir_all(source_root.join("nested")).unwrap();
        std::fs::write(source_root.join("nested/one.mkv"), b"one").unwrap();
        std::fs::write(source_root.join("two.mkv"), b"two").unwrap();
        let managed_root = downloads.path().join("Directory Torrent");
        copy_dir_all(&source_root, &managed_root);
        std::fs::write(managed_root.join("unrelated.txt"), b"do not delete").unwrap();
        let torrent_bytes = make_fixture(&source_root).await;

        let make_session = || {
            Session::new_with_opts(
                downloads.path().to_path_buf(),
                SessionOptions {
                    dht: None,
                    listen: None,
                    disable_local_service_discovery: true,
                    persistence: Some(SessionPersistenceConfig::Json {
                        folder: Some(persistence.path().to_path_buf()),
                    }),
                    ..Default::default()
                },
            )
        };
        let session = make_session().await.unwrap();
        session
            .create_automation_category("radarr".to_owned())
            .await
            .unwrap();
        let torrent = session
            .add_torrent(
                AddTorrent::from_bytes(torrent_bytes),
                Some(AddTorrentOptions {
                    paused: true,
                    overwrite: true,
                    automation: TorrentAutomationMetadata {
                        category: "radarr".to_owned(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            )
            .await
            .unwrap()
            .into_handle()
            .unwrap();
        session
            .delete(TorrentIdOrHash::Hash(torrent.info_hash()), true)
            .await
            .unwrap();
        assert!(!managed_root.join("nested/one.mkv").exists());
        assert!(!managed_root.join("two.mkv").exists());
        assert_eq!(
            std::fs::read(managed_root.join("unrelated.txt")).unwrap(),
            b"do not delete"
        );
        session.stop().await;
        drop(session);

        let restored = make_session().await.unwrap();
        let torrent_count = restored.with_torrents(|torrents| torrents.count());
        assert_eq!(torrent_count, 0);
        restored.stop().await;
    }

    #[tokio::test]
    async fn multipart_add_rejects_path_traversal() {
        let downloads = tempfile::tempdir().unwrap();
        let session = Session::new_with_opts(
            downloads.path().to_path_buf(),
            SessionOptions {
                dht: None,
                listen: None,
                disable_local_service_discovery: true,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let state = Arc::new(HttpApi::new(
            Api::new(session, None, None),
            Some(HttpApiOptions {
                enable_qbittorrent_api: true,
                ..Default::default()
            }),
        ));
        let (base, server) = start_server(make_api_router(state)).await;
        let malicious = b"d4:infod5:filesld6:lengthi1e4:pathl2:..10:escape.mkveee4:name4:root12:piece lengthi1e6:pieces20:aaaaaaaaaaaaaaaaaaaaee";
        let boundary = "rqbit-path-traversal-boundary";
        let response = reqwest::Client::new()
            .post(format!("{base}/torrents/add"))
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(multipart_body(boundary, "", "malicious.torrent", malicious))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        assert!(
            !downloads
                .path()
                .parent()
                .unwrap()
                .join("escape.mkv")
                .exists()
        );
        server.abort();
    }

    fn copy_dir_all(source: &Path, destination: &Path) {
        std::fs::create_dir_all(destination).unwrap();
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            let destination = destination.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir_all(&entry.path(), &destination);
            } else {
                std::fs::copy(entry.path(), destination).unwrap();
            }
        }
    }
}
