use std::{net::SocketAddr, str::FromStr, time::Duration};

use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::{AddTorrentOptions, PeerConnectionOptions};

pub struct OnlyFiles(Vec<usize>);
pub struct InitialPeers(pub Vec<SocketAddr>);

pub use crate::torrent_state::peer::stats::snapshot::{PeerStatsFilter, PeerStatsSnapshot};

#[derive(Serialize, Deserialize, Default)]
pub struct TorrentAddQueryParams {
    pub overwrite: Option<bool>,
    pub output_folder: Option<String>,
    pub sub_folder: Option<String>,
    pub only_files_regex: Option<String>,
    pub only_files: Option<OnlyFiles>,
    pub peer_connect_timeout: Option<u64>,
    pub peer_read_write_timeout: Option<u64>,
    pub initial_peers: Option<InitialPeers>,
    // Will force interpreting the content as a URL.
    pub is_url: Option<bool>,
    pub list_only: Option<bool>,
}

impl Serialize for OnlyFiles {
    fn serialize<S>(&self, serializer: S) -> core::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = self.0.iter().map(|id| id.to_string()).join(",");
        s.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OnlyFiles {
    fn deserialize<D>(deserializer: D) -> core::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let s = String::deserialize(deserializer)?;
        let list = s
            .split(',')
            .try_fold(Vec::<usize>::new(), |mut acc, c| match c.parse() {
                Ok(i) => {
                    acc.push(i);
                    Ok(acc)
                }
                Err(_) => Err(D::Error::custom(format!(
                    "only_files: failed to parse {c:?} as integer"
                ))),
            })?;
        if list.is_empty() {
            return Err(D::Error::custom(
                "only_files: should contain at least one file id",
            ));
        }
        Ok(OnlyFiles(list))
    }
}

impl<'de> Deserialize<'de> for InitialPeers {
    fn deserialize<D>(deserializer: D) -> std::prelude::v1::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;
        let string = String::deserialize(deserializer)?;
        let mut addrs = Vec::new();
        for addr_str in string.split(',').filter(|s| !s.is_empty()) {
            addrs.push(SocketAddr::from_str(addr_str).map_err(D::Error::custom)?);
        }
        Ok(InitialPeers(addrs))
    }
}

impl Serialize for InitialPeers {
    fn serialize<S>(&self, serializer: S) -> std::prelude::v1::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0
            .iter()
            .map(|s| s.to_string())
            .join(",")
            .serialize(serializer)
    }
}

impl TorrentAddQueryParams {
    pub fn into_add_torrent_options(self) -> AddTorrentOptions {
        AddTorrentOptions {
            overwrite: self.overwrite.unwrap_or(false),
            only_files_regex: self.only_files_regex,
            only_files: self.only_files.map(|o| o.0),
            output_folder: self.output_folder,
            sub_folder: self.sub_folder,
            list_only: self.list_only.unwrap_or(false),
            initial_peers: self.initial_peers.map(|i| i.0),
            peer_opts: Some(PeerConnectionOptions {
                connect_timeout: self.peer_connect_timeout.map(Duration::from_secs),
                read_write_timeout: self.peer_read_write_timeout.map(Duration::from_secs),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, time::Duration};

    use super::{InitialPeers, OnlyFiles, TorrentAddQueryParams};

    #[test]
    fn only_files_round_trips_as_comma_separated_ids() {
        let encoded = serde_json::to_string(&OnlyFiles(vec![0, 2, 10])).unwrap();
        assert_eq!(encoded, "\"0,2,10\"");
        let decoded: OnlyFiles = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.0, vec![0, 2, 10]);
    }

    #[test]
    fn only_files_rejects_empty_and_invalid_values() {
        assert!(serde_json::from_str::<OnlyFiles>("\"\"").is_err());
        assert!(serde_json::from_str::<OnlyFiles>("\"1,nope\"").is_err());
    }

    #[test]
    fn initial_peers_round_trip_ipv4_and_ipv6() {
        let peers = InitialPeers(vec![
            "127.0.0.1:1".parse().unwrap(),
            "[::1]:2".parse().unwrap(),
        ]);
        let encoded = serde_json::to_string(&peers).unwrap();
        let decoded: InitialPeers = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.0, peers.0);
    }

    #[test]
    fn initial_peers_accepts_empty_input_and_rejects_bad_addresses() {
        let empty: InitialPeers = serde_json::from_str("\"\"").unwrap();
        assert!(empty.0.is_empty());
        assert!(serde_json::from_str::<InitialPeers>("\"localhost:1\"").is_err());
    }

    #[test]
    fn query_params_convert_all_public_options() {
        let peer: SocketAddr = "127.0.0.1:6881".parse().unwrap();
        let options = TorrentAddQueryParams {
            overwrite: Some(true),
            output_folder: Some("out".into()),
            sub_folder: Some("sub".into()),
            only_files_regex: Some("video".into()),
            only_files: Some(OnlyFiles(vec![1, 3])),
            peer_connect_timeout: Some(7),
            peer_read_write_timeout: Some(11),
            initial_peers: Some(InitialPeers(vec![peer])),
            is_url: Some(true),
            list_only: Some(true),
        }
        .into_add_torrent_options();

        assert!(options.overwrite);
        assert!(options.list_only);
        assert_eq!(options.output_folder.as_deref(), Some("out"));
        assert_eq!(options.sub_folder.as_deref(), Some("sub"));
        assert_eq!(options.only_files_regex.as_deref(), Some("video"));
        assert_eq!(options.only_files, Some(vec![1, 3]));
        assert_eq!(options.initial_peers, Some(vec![peer]));
        let peer_options = options.peer_opts.unwrap();
        assert_eq!(peer_options.connect_timeout, Some(Duration::from_secs(7)));
        assert_eq!(
            peer_options.read_write_timeout,
            Some(Duration::from_secs(11))
        );
    }

    #[test]
    fn query_defaults_match_add_torrent_defaults() {
        let options = TorrentAddQueryParams::default().into_add_torrent_options();
        assert!(!options.overwrite);
        assert!(!options.list_only);
        assert_eq!(options.only_files, None);
        assert_eq!(options.initial_peers, None);
        assert!(options.peer_opts.is_some());
    }
}
