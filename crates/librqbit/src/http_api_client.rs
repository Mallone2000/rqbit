use anyhow::Context;
use futures::{FutureExt, future::BoxFuture};
use serde::Deserialize;

use crate::{
    api::ApiAddTorrentResponse,
    http_api_types::{InitialPeers, TorrentAddQueryParams},
    session::{AddTorrent, AddTorrentOptions},
};

const MAX_API_RESPONSE_SIZE: usize = 64 * 1024 * 1024;
const MAX_API_ERROR_SIZE: usize = 64 * 1024;

async fn response_bytes_limited(
    mut response: reqwest::Response,
    max_size: usize,
) -> anyhow::Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > max_size as u64)
    {
        anyhow::bail!("HTTP API response exceeds {max_size} bytes")
    }
    let mut body = Vec::with_capacity(
        response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or_default()
            .min(max_size),
    );
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(reqwest::Error::without_url)?
    {
        if body.len().saturating_add(chunk.len()) > max_size {
            anyhow::bail!("HTTP API response exceeds {max_size} bytes")
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[derive(Clone)]
pub struct HttpApiClient {
    client: reqwest::Client,
    base_url: reqwest::Url,
}

async fn check_response(r: reqwest::Response) -> anyhow::Result<reqwest::Response> {
    if r.status().is_success() {
        return Ok(r);
    }
    let status = r.status();
    let url = crate::redact_url_for_logging(r.url().as_str());
    let body = response_bytes_limited(r, MAX_API_ERROR_SIZE)
        .await
        .with_context(|| format!("cannot read response body for request to {url} ({status})"))?;
    let body = String::from_utf8_lossy(&body);

    #[derive(Deserialize)]
    struct HumanReadableError<'a> {
        human_readable: Option<&'a str>,
    }

    let human_readable_internal_error = serde_json::from_str::<HumanReadableError<'_>>(&body)
        .ok()
        .and_then(|e| e.human_readable);
    let body_display = human_readable_internal_error.unwrap_or(&body);

    anyhow::bail!("{} -> {}: {}", url, status, body_display)
}

#[derive(Deserialize)]
struct ApiRoot {
    server: String,
}

async fn json_response<T: serde::de::DeserializeOwned + std::any::Any>(
    response: reqwest::Response,
) -> anyhow::Result<T> {
    let url = crate::redact_url_for_logging(response.url().as_str());
    let response = check_response(response).await?;
    let body = response_bytes_limited(response, MAX_API_RESPONSE_SIZE).await?;
    let response: T = serde_json::from_slice(&body).with_context(|| {
        format!(
            "error deserializing response from {:?} as {:?}",
            url,
            std::any::type_name::<T>(),
        )
    })?;
    Ok(response)
}

impl HttpApiClient {
    #[inline(never)]
    pub fn new(url: &str) -> anyhow::Result<Self> {
        Ok(Self {
            base_url: reqwest::Url::parse(url)?,
            client: reqwest::ClientBuilder::new().build()?,
        })
    }

    pub fn base_url(&self) -> &reqwest::Url {
        &self.base_url
    }

    #[inline(never)]
    pub fn validate_rqbit_server(&self) -> BoxFuture<'_, anyhow::Result<()>> {
        async move {
            let response = self
                .client
                .get(self.base_url.clone())
                .send()
                .await
                .map_err(reqwest::Error::without_url)?;
            let root: ApiRoot = json_response(response).await?;
            if root.server == "rqbit" {
                return Ok(());
            }
            anyhow::bail!(
                "not an rqbit server at {}",
                crate::redact_url_for_logging(self.base_url.as_str())
            )
        }
        .boxed()
    }

    pub fn add_torrent<'a>(
        &'a self,
        torrent: AddTorrent<'a>,
        opts: Option<AddTorrentOptions>,
    ) -> BoxFuture<'a, anyhow::Result<ApiAddTorrentResponse>> {
        async move {
            let opts = opts.unwrap_or_default();
            let params = TorrentAddQueryParams {
                overwrite: Some(opts.overwrite),
                only_files_regex: opts.only_files_regex,
                only_files: None,
                output_folder: opts.output_folder,
                sub_folder: opts.sub_folder,
                list_only: Some(opts.list_only),
                initial_peers: opts.initial_peers.map(InitialPeers),
                ..Default::default()
            };
            let qs = serde_urlencoded::to_string(&params).unwrap();
            let url = format!("{}torrents?{}", self.base_url, qs);
            let response = check_response(
                self.client
                    .post(&url)
                    .body(torrent.into_bytes())
                    .send()
                    .await
                    .map_err(reqwest::Error::without_url)?,
            )
            .await?;
            json_response(response).await
        }
        .boxed()
    }
}
