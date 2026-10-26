use serde::{Deserialize, Serialize};

/// Client-agnostic metadata used by download automation integrations.
///
/// Protocol-specific names and states belong in their HTTP compatibility
/// layers. This type contains only concepts that are meaningful to rqbit.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct TorrentAutomationMetadata {
    #[serde(default)]
    pub category: String,
    /// qBittorrent tags attached to this torrent. Stored with pending magnets too.
    #[serde(default)]
    pub tags: std::collections::BTreeSet<String>,
    #[serde(default)]
    pub ratio_limit: Option<f64>,
    /// Active completed time limit, stored in seconds. Protocol adapters may
    /// expose a different unit.
    #[serde(default)]
    pub seeding_time_limit_seconds: Option<u64>,
    #[serde(default)]
    pub added_at_unix_seconds: u64,
    #[serde(default)]
    pub completed_at_unix_seconds: Option<u64>,
    #[serde(default)]
    pub uploaded_bytes: u64,
    /// qBittorrent-compatible finished duration: monotonic elapsed time while
    /// the torrent is complete and live. Idle/no-peer time counts; paused time
    /// does not. The accumulated seconds are persisted across restarts.
    #[serde(default)]
    pub seeding_seconds: u64,
    #[serde(default)]
    pub last_activity_unix_seconds: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct AutomationCategory {
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::TorrentAutomationMetadata;

    #[test]
    fn older_automation_metadata_defaults_tags() {
        let metadata: TorrentAutomationMetadata =
            serde_json::from_str(r#"{"category":"sonarr"}"#).unwrap();
        assert!(metadata.tags.is_empty());
        let mut tagged = metadata;
        tagged.tags.insert("cleanup".to_owned());
        assert!(serde_json::to_string(&tagged).unwrap().contains("cleanup"));
    }
}
