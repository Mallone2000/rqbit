use serde::Serialize;

#[derive(Serialize)]
pub(super) struct Preferences {
    pub save_path: String,
    pub dht: bool,
    pub queueing_enabled: bool,
    pub max_ratio_enabled: bool,
    pub max_ratio: f64,
    pub max_ratio_act: u8,
    pub max_seeding_time_enabled: bool,
    pub max_seeding_time: u64,
    pub max_inactive_seeding_time_enabled: bool,
    pub max_inactive_seeding_time: u64,
}

#[derive(Serialize)]
pub(super) struct Category {
    pub name: String,
    #[serde(rename = "savePath")]
    pub save_path: String,
}

#[derive(Serialize)]
pub(super) struct TorrentInfo {
    pub hash: String,
    pub name: String,
    pub size: u64,
    pub progress: f64,
    pub eta: u64,
    pub state: &'static str,
    pub label: String,
    pub category: String,
    pub save_path: String,
    pub content_path: String,
    pub ratio: f64,
    pub ratio_limit: f64,
    pub seeding_time: u64,
    pub seeding_time_limit: i64,
    pub inactive_seeding_time_limit: i64,
    pub last_activity: u64,
}

#[derive(Serialize)]
pub(super) struct TorrentProperties {
    pub save_path: String,
    pub total_size: u64,
    pub total_downloaded: u64,
    pub total_uploaded: u64,
    pub total_wasted: u64,
    pub time_elapsed: u64,
    pub seeding_time: u64,
    pub nb_connections: u64,
    pub share_ratio: f64,
    pub addition_date: u64,
    pub completion_date: u64,
    pub created_by: String,
    pub dl_limit: i64,
    pub up_limit: i64,
    pub piece_size: u32,
}

#[derive(Serialize)]
pub(super) struct TorrentFile {
    pub index: usize,
    pub name: String,
    pub size: u64,
    pub progress: f64,
    pub priority: u8,
    pub is_seed: bool,
}
