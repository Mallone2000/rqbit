use anyhow::Context;
use axum::{extract::State, response::IntoResponse};
use bencode::AsDisplay;
use buffers::ByteBuf;
use http::{HeaderMap, HeaderValue, StatusCode};

use super::ApiState;
use crate::{
    AddTorrent, AddTorrentOptions, ListOnlyResponse, WithStatusError, api::Result,
    http_api::timeout::Timeout,
};

const MAX_MAGNET_URL_SIZE: usize = 16 * 1024;

fn sanitize_magnet_url(url: &str) -> Result<String> {
    if url.len() > MAX_MAGNET_URL_SIZE {
        return Err((StatusCode::PAYLOAD_TOO_LARGE, "magnet URL is too large").into());
    }
    let mut magnet = librqbit_core::magnet::Magnet::parse(url)
        .with_status_error(StatusCode::BAD_REQUEST, "invalid magnet URL")?;
    // This endpoint is also exposed by read-only servers. Do not let a caller
    // turn it into an HTTP/UDP proxy for arbitrary tracker addresses.
    magnet.trackers.clear();
    Ok(magnet.to_string())
}

pub async fn h_resolve_magnet(
    State(state): State<ApiState>,
    Timeout(timeout): Timeout<600_000, 3_600_000>,
    inp_headers: HeaderMap,
    url: String,
) -> Result<impl IntoResponse> {
    let url = sanitize_magnet_url(&url)?;
    let added = tokio::time::timeout(
        timeout,
        state.api.session().add_torrent(
            AddTorrent::from_url(&url),
            Some(AddTorrentOptions {
                list_only: true,
                ..Default::default()
            }),
        ),
    )
    .await
    .context("timeout")??;

    let (info, content) = match added {
        crate::AddTorrentResponse::AlreadyManaged(_, handle) => {
            handle.with_metadata(|r| (r.info.clone(), r.torrent_bytes.clone()))?
        }
        crate::AddTorrentResponse::ListOnly(ListOnlyResponse {
            info,
            torrent_bytes,
            ..
        }) => (info, torrent_bytes),
        crate::AddTorrentResponse::Added(_, _) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "bug: torrent was added to session, but shouldn't have been",
            )
                .into());
        }
    };

    let mut headers = HeaderMap::new();

    if inp_headers
        .get("Accept")
        .and_then(|v| std::str::from_utf8(v.as_bytes()).ok())
        == Some("application/json")
    {
        let data = bencode::dyn_from_bytes::<AsDisplay<ByteBuf>>(&content)
            .map_err(|e| {
                tracing::trace!("error decoding .torrent file content: {e:#}");
                e.into_kind()
            })
            .context("error decoding .torrent file content")?;
        let data = serde_json::to_string(&data).context("error serializing")?;
        headers.insert("Content-Type", HeaderValue::from_static("application/json"));
        return Ok((headers, data).into_response());
    }

    headers.insert(
        "Content-Type",
        HeaderValue::from_static("application/x-bittorrent"),
    );

    if let Some(name) = info.name()
        && let Ok(h) = HeaderValue::from_str(&format!("attachment; filename=\"{name}.torrent\""))
    {
        headers.insert("Content-Disposition", h);
    }
    Ok((headers, content).into_response())
}

#[cfg(test)]
mod tests {
    use super::sanitize_magnet_url;

    #[test]
    fn resolve_magnet_rejects_non_magnet_and_oversized_urls() {
        assert!(
            sanitize_magnet_url("magnet:?xt=urn:btih:0000000000000000000000000000000000000001")
                .is_ok()
        );
        assert!(sanitize_magnet_url("http://127.0.0.1/private").is_err());
        assert!(sanitize_magnet_url(&"x".repeat(16 * 1024 + 1)).is_err());

        let sanitized = sanitize_magnet_url(
            "magnet:?xt=urn:btih:0000000000000000000000000000000000000001&tr=http%3A%2F%2F127.0.0.1%2Fprivate",
        )
        .unwrap();
        assert!(!sanitized.contains("tr="));
        assert!(!sanitized.contains("127.0.0.1"));
    }
}
