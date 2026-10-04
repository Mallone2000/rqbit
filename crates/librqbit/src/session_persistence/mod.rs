pub mod json;
#[cfg(feature = "postgres")]
pub mod postgres;

use std::{collections::HashSet, path::PathBuf};

use anyhow::Context;
use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::BoxStream;
use librqbit_core::Id20;
use librqbit_core::magnet::Magnet;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    AddTorrent, AddTorrentOptions, AutomationCategory, TorrentAutomationMetadata,
    bitv_factory::BitVFactory, session::TorrentId, torrent_state::ManagedTorrentHandle,
};

#[derive(Serialize, Deserialize, Clone)]
pub struct SerializedTorrent {
    #[serde(
        serialize_with = "serialize_info_hash",
        deserialize_with = "deserialize_info_hash"
    )]
    info_hash: Id20,
    #[serde(skip)]
    torrent_bytes: Bytes,
    trackers: HashSet<String>,
    output_folder: PathBuf,
    only_files: Option<Vec<usize>>,
    is_paused: bool,
    #[serde(default)]
    automation: TorrentAutomationMetadata,
}

impl SerializedTorrent {
    pub fn info_hash(&self) -> &Id20 {
        &self.info_hash
    }
    pub fn into_add_torrent(self) -> anyhow::Result<(AddTorrent<'static>, AddTorrentOptions)> {
        let add_torrent = if !self.torrent_bytes.is_empty() {
            AddTorrent::TorrentFileBytes(self.torrent_bytes)
        } else {
            let magnet = Magnet::from_id20(
                self.info_hash,
                self.trackers.into_iter().collect(),
                self.only_files.clone(),
            )
            .to_string();
            AddTorrent::from_url(magnet)
        };

        let opts = AddTorrentOptions {
            paused: self.is_paused,
            output_folder: Some(
                self.output_folder
                    .to_str()
                    .context("broken path")?
                    .to_owned(),
            ),
            only_files: self.only_files,
            overwrite: true,
            automation: self.automation,
            ..Default::default()
        };

        Ok((add_torrent, opts))
    }
}

/// Durable submission for an automation magnet whose metadata is not available yet.
/// The payload can contain private tracker credentials and must never be logged.
#[derive(Clone, Serialize, Deserialize)]
pub struct SerializedPendingTorrent {
    #[serde(
        serialize_with = "serialize_info_hash",
        deserialize_with = "deserialize_info_hash"
    )]
    pub(crate) info_hash: Id20,
    pub(crate) magnet_url: String,
    pub(crate) options: serde_json::Value,
    #[serde(default)]
    pub(crate) paused: bool,
    #[serde(default)]
    pub(crate) state: crate::session::PendingAutomationTorrentState,
    #[serde(default)]
    pub(crate) metadata_attempts: u32,
    #[serde(default)]
    pub(crate) last_error: Option<crate::session::PendingAutomationError>,
    #[serde(default)]
    pub(crate) next_retry_at_unix_seconds: Option<u64>,
}

impl SerializedPendingTorrent {
    pub fn info_hash(&self) -> &Id20 {
        &self.info_hash
    }
}

impl std::fmt::Debug for SerializedPendingTorrent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SerializedPendingTorrent")
            .field("info_hash", &self.info_hash)
            .field("paused", &self.paused)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

// TODO: make this info_hash first, ID-second.
#[async_trait]
pub trait SessionPersistenceStore: core::fmt::Debug + Send + Sync + BitVFactory {
    async fn load_pending(&self) -> anyhow::Result<Vec<SerializedPendingTorrent>>;
    async fn store_pending(&self, torrent: &SerializedPendingTorrent) -> anyhow::Result<()>;
    async fn delete_pending(&self, info_hash: Id20) -> anyhow::Result<()>;
    async fn next_id(&self) -> anyhow::Result<TorrentId>;
    async fn store(&self, id: TorrentId, torrent: &ManagedTorrentHandle) -> anyhow::Result<()>;
    async fn delete(&self, id: TorrentId) -> anyhow::Result<()>;
    async fn get(&self, id: TorrentId) -> anyhow::Result<SerializedTorrent>;
    async fn update_metadata(
        &self,
        id: TorrentId,
        torrent: &ManagedTorrentHandle,
    ) -> anyhow::Result<()>;
    async fn load_automation_categories(&self) -> anyhow::Result<Vec<AutomationCategory>>;
    async fn store_automation_category(&self, category: &AutomationCategory) -> anyhow::Result<()>;
    async fn delete_automation_category(&self, name: &str) -> anyhow::Result<()>;
    async fn stream_all(
        &self,
    ) -> anyhow::Result<BoxStream<'_, anyhow::Result<(TorrentId, SerializedTorrent)>>>;
}

fn serialize_info_hash<S>(id: &Id20, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    id.as_string().serialize(serializer)
}

fn deserialize_info_hash<'de, D>(deserializer: D) -> Result<Id20, D::Error>
where
    D: Deserializer<'de>,
{
    Id20::deserialize(deserializer)
}
