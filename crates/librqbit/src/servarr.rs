use std::time::Duration;

use anyhow::{Context, bail};
use librqbit_core::Id20;
use reqwest::header::HeaderValue;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServarrKind {
    Sonarr,
    Radarr,
}

/// Credentials and category routing for one Sonarr or Radarr instance.
/// Keep this out of Debug output because it contains an API key.
pub struct ServarrConfig {
    pub kind: ServarrKind,
    pub category: String,
    pub url: String,
    pub api_key: String,
}

pub(crate) struct ServarrService {
    pub category: String,
    pub kind: ServarrKind,
    base_url: Url,
    api_key: HeaderValue,
    client: reqwest::Client,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueuePage {
    #[serde(default)]
    records: Option<Vec<QueueItem>>,
    total_records: usize,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueueItem {
    pub id: i64,
    pub download_id: Option<String>,
    #[serde(default)]
    pub movie_id: Option<i64>,
    #[serde(default)]
    pub episode_ids: Vec<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct ReplacementPlan {
    pub queue_id: i64,
    pub target: ReplacementTarget,
    #[serde(default)]
    pub blocklisted: bool,
    #[serde(default)]
    pub search_triggered: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum ReplacementTarget {
    SonarrEpisodes(Vec<i64>),
    RadarrMovie(i64),
}

impl ServarrService {
    pub fn new(config: ServarrConfig) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !config.category.is_empty(),
            "Servarr category cannot be empty"
        );
        anyhow::ensure!(
            !config.api_key.is_empty(),
            "Servarr API key cannot be empty"
        );
        let base_url = Url::parse(&config.url).context("invalid Servarr URL")?;
        anyhow::ensure!(
            matches!(base_url.scheme(), "http" | "https")
                && base_url.host().is_some()
                && base_url.username().is_empty()
                && base_url.password().is_none()
                && base_url.query().is_none()
                && base_url.fragment().is_none(),
            "Servarr URL must be an HTTP(S) base URL without credentials or query parameters"
        );
        let mut api_key =
            HeaderValue::from_str(&config.api_key).context("invalid Servarr API key")?;
        api_key.set_sensitive(true);
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .build()
            .context("could not create Servarr HTTP client")?;
        Ok(Self {
            category: config.category,
            kind: config.kind,
            base_url,
            api_key,
            client,
        })
    }

    fn endpoint(&self, components: &[&str]) -> Url {
        let mut url = self.base_url.clone();
        url.path_segments_mut()
            .expect("HTTP(S) URLs have path segments")
            .pop_if_empty()
            .extend(["api", "v3"].into_iter().chain(components.iter().copied()));
        url
    }

    pub async fn find_queue_item(&self, hash: Id20) -> anyhow::Result<Option<QueueItem>> {
        const PAGE_SIZE: usize = 100;
        for page in 1..=100 {
            let mut url = self.endpoint(&["queue"]);
            url.query_pairs_mut()
                .append_pair("page", &page.to_string())
                .append_pair("pageSize", &PAGE_SIZE.to_string());
            let response = self
                .client
                .get(url)
                .header("X-Api-Key", self.api_key.clone())
                .send()
                .await
                .context("could not query Servarr queue")?
                .error_for_status()
                .context("Servarr queue returned an error")?;
            let listing: QueuePage = response
                .json()
                .await
                .context("invalid Servarr queue response")?;
            let records = listing.records.unwrap_or_default();
            if let Some(item) = records.iter().find(|item| {
                item.download_id
                    .as_deref()
                    .is_some_and(|download_id| download_id.eq_ignore_ascii_case(&hash.as_string()))
            }) {
                return Ok(Some(item.clone()));
            }
            if records.is_empty() || page * PAGE_SIZE >= listing.total_records {
                return Ok(None);
            }
        }
        bail!("Servarr queue exceeded pagination limit")
    }

    pub fn plan(&self, item: QueueItem) -> anyhow::Result<ReplacementPlan> {
        anyhow::ensure!(item.id > 0, "invalid Servarr queue ID");
        let target = match self.kind {
            ServarrKind::Sonarr if !item.episode_ids.is_empty() => {
                anyhow::ensure!(
                    item.episode_ids.iter().all(|id| *id > 0),
                    "invalid Sonarr episode IDs"
                );
                ReplacementTarget::SonarrEpisodes(item.episode_ids)
            }
            ServarrKind::Radarr => match item.movie_id {
                Some(id) if id > 0 => ReplacementTarget::RadarrMovie(id),
                _ => bail!("Radarr queue item has no movie ID"),
            },
            _ => bail!("Servarr queue item has no searchable media ID"),
        };
        Ok(ReplacementPlan {
            queue_id: item.id,
            target,
            blocklisted: false,
            search_triggered: false,
        })
    }

    pub async fn blocklist(&self, queue_id: i64) -> anyhow::Result<()> {
        let mut url = self.endpoint(&["queue", &queue_id.to_string()]);
        url.query_pairs_mut()
            .append_pair("removeFromClient", "false")
            .append_pair("blocklist", "true")
            .append_pair("skipRedownload", "true");
        let response = self
            .client
            .delete(url)
            .header("X-Api-Key", self.api_key.clone())
            .send()
            .await
            .context("could not blocklist Servarr queue item")?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            // A prior request may have succeeded before its response or persistence write failed.
            return Ok(());
        }
        response
            .error_for_status()
            .context("Servarr rejected queue blocklisting")?;
        Ok(())
    }

    pub async fn trigger_search(&self, target: &ReplacementTarget) -> anyhow::Result<()> {
        let body = match (self.kind, target) {
            (ServarrKind::Sonarr, ReplacementTarget::SonarrEpisodes(episode_ids)) => {
                serde_json::json!({"name": "EpisodeSearch", "episodeIds": episode_ids})
            }
            (ServarrKind::Radarr, ReplacementTarget::RadarrMovie(movie_id)) => {
                serde_json::json!({"name": "MoviesSearch", "movieIds": [movie_id]})
            }
            _ => bail!("replacement target does not match Servarr service"),
        };
        self.client
            .post(self.endpoint(&["command"]))
            .header("X-Api-Key", self.api_key.clone())
            .json(&body)
            .send()
            .await
            .context("could not request Servarr replacement search")?
            .error_for_status()
            .context("Servarr rejected replacement search")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::{QueueItem, ReplacementTarget, ServarrConfig, ServarrKind, ServarrService};

    #[test]
    fn rejects_ambiguous_or_unsafe_service_configuration() {
        for url in [
            "http://user:password@localhost:8989",
            "http://localhost:8989/?apikey=secret",
            "file:///tmp/servarr",
        ] {
            assert!(
                ServarrService::new(ServarrConfig {
                    kind: ServarrKind::Sonarr,
                    category: "sonarr".to_owned(),
                    url: url.to_owned(),
                    api_key: "example-key".to_owned(),
                })
                .is_err()
            );
        }
    }

    #[test]
    fn search_plan_targets_the_corresponding_media() {
        let sonarr = ServarrService::new(ServarrConfig {
            kind: ServarrKind::Sonarr,
            category: "sonarr".to_owned(),
            url: "http://localhost:8989/sonarr".to_owned(),
            api_key: "example-key".to_owned(),
        })
        .unwrap();
        assert_eq!(
            sonarr.endpoint(&["queue"]).as_str(),
            "http://localhost:8989/sonarr/api/v3/queue"
        );
        let item = QueueItem {
            id: 17,
            download_id: None,
            movie_id: None,
            episode_ids: vec![42, 43],
        };
        let plan = sonarr.plan(item.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(plan.target).unwrap(),
            serde_json::json!({"SonarrEpisodes": [42, 43]})
        );
        let radarr = ServarrService::new(ServarrConfig {
            kind: ServarrKind::Radarr,
            category: "radarr".to_owned(),
            url: "http://localhost:7878".to_owned(),
            api_key: "example-key".to_owned(),
        })
        .unwrap();
        assert!(radarr.plan(item.clone()).is_err());
        let plan = radarr
            .plan(QueueItem {
                movie_id: Some(91),
                ..item
            })
            .unwrap();
        assert_eq!(
            serde_json::to_value(plan.target).unwrap(),
            serde_json::json!({"RadarrMovie": 91})
        );
    }

    #[tokio::test]
    async fn radarr_search_posts_the_movie_id() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0; 4096];
                let count = stream.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&chunk[..count]);
                if bytes.windows(4).any(|window| window == b"\r\n\r\n")
                    && bytes
                        .windows(b"\"movieIds\":[91]".len())
                        .any(|window| window == b"\"movieIds\":[91]")
                {
                    break;
                }
            }
            let request = String::from_utf8(bytes).unwrap();
            assert!(request.starts_with("POST /api/v3/command "));
            assert!(request.contains("\"name\":\"MoviesSearch\""));
            stream
                .write_all(
                    b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                )
                .await
                .unwrap();
        });
        let service = ServarrService::new(ServarrConfig {
            kind: ServarrKind::Radarr,
            category: "radarr".to_owned(),
            url: format!("http://{address}"),
            api_key: "example-key".to_owned(),
        })
        .unwrap();
        service
            .trigger_search(&ReplacementTarget::RadarrMovie(91))
            .await
            .unwrap();
        server.await.unwrap();
    }
}
