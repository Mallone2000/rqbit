use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::{Path, PathBuf},
    time::Duration,
};

use librqbit::{
    ConnectionOptions, ListenerMode, ListenerOptions, PeerConnectionOptions, dht::PersistentDht,
    limits::LimitsConfig,
};
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct RqbitDesktopConfigDht {
    pub disable: bool,
    pub disable_persistence: bool,
    pub persistence_filename: PathBuf,
}

impl Default for RqbitDesktopConfigDht {
    fn default() -> Self {
        Self {
            disable: false,
            disable_persistence: false,
            persistence_filename: PersistentDht::default_persistence_filename().unwrap(),
        }
    }
}

#[serde_as]
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct RqbitDesktopConfigConnections {
    pub enable_tcp_listen: bool,
    pub enable_tcp_outgoing: bool,
    pub enable_utp: bool,
    pub enable_upnp_port_forward: bool,
    pub socks_proxy: String,
    pub listen_port: u16,

    #[serde_as(as = "serde_with::DurationSeconds")]
    pub peer_connect_timeout: Duration,
    #[serde_as(as = "serde_with::DurationSeconds")]
    pub peer_read_write_timeout: Duration,
}

impl RqbitDesktopConfigConnections {
    pub fn as_listener_and_connect_opts(&self) -> (Option<ListenerOptions>, ConnectionOptions) {
        let mode = match (self.enable_tcp_listen, self.enable_utp) {
            (true, true) => Some(ListenerMode::TcpAndUtp),
            (true, false) => Some(ListenerMode::TcpOnly),
            (false, true) => Some(ListenerMode::UtpOnly),
            (false, false) => None,
        };
        let listener_opts = mode.map(|mode| ListenerOptions {
            mode,
            listen_addr: (Ipv4Addr::UNSPECIFIED, self.listen_port).into(),
            enable_upnp_port_forwarding: self.enable_upnp_port_forward,
            ..Default::default()
        });
        let connect_opts = ConnectionOptions {
            proxy_url: if self.socks_proxy.is_empty() {
                None
            } else {
                Some(self.socks_proxy.clone())
            },
            enable_tcp: self.enable_tcp_outgoing,
            peer_opts: Some(PeerConnectionOptions {
                connect_timeout: Some(self.peer_connect_timeout),
                read_write_timeout: Some(self.peer_read_write_timeout),
                ..Default::default()
            }),
        };
        (listener_opts, connect_opts)
    }
}

impl Default for RqbitDesktopConfigConnections {
    fn default() -> Self {
        Self {
            enable_tcp_listen: true,
            enable_tcp_outgoing: true,
            enable_utp: false,
            enable_upnp_port_forward: true,
            listen_port: 4240,
            socks_proxy: String::new(),
            peer_connect_timeout: Duration::from_secs(2),
            peer_read_write_timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct RqbitDesktopConfigPersistence {
    pub disable: bool,

    #[serde(default)]
    pub folder: PathBuf,

    #[serde(default)]
    pub fastresume: bool,

    /// Deprecated, but keeping for backwards compat for serialized / deserialized config.
    #[serde(default)]
    pub filename: PathBuf,
}

impl RqbitDesktopConfigPersistence {
    pub(crate) fn fix_backwards_compat(&mut self) {
        if self.folder != Path::new("") {
            return;
        }
        if self.filename != Path::new("")
            && let Some(parent) = self.filename.parent()
        {
            self.folder = parent.to_owned();
        }
    }
}

impl Default for RqbitDesktopConfigPersistence {
    fn default() -> Self {
        let folder = librqbit::SessionPersistenceConfig::default_json_persistence_folder().unwrap();
        Self {
            disable: false,
            folder,
            fastresume: false,
            filename: PathBuf::new(),
        }
    }
}

#[serde_as]
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct RqbitDesktopConfigHttpApi {
    pub disable: bool,
    pub listen_addr: SocketAddr,
    pub read_only: bool,
}

impl Default for RqbitDesktopConfigHttpApi {
    fn default() -> Self {
        Self {
            disable: Default::default(),
            listen_addr: SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 3030)),
            read_only: false,
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct RqbitDesktopConfigUpnp {
    #[serde(default)]
    pub enable_server: bool,

    #[serde(default)]
    pub server_friendly_name: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct RqbitDesktopConfig {
    pub default_download_location: PathBuf,

    #[cfg(feature = "disable-upload")]
    #[serde(default)]
    pub disable_upload: bool,

    pub dht: RqbitDesktopConfigDht,
    pub connections: RqbitDesktopConfigConnections,
    pub upnp: RqbitDesktopConfigUpnp,
    pub persistence: RqbitDesktopConfigPersistence,
    pub http_api: RqbitDesktopConfigHttpApi,

    #[serde(default)]
    pub ratelimits: LimitsConfig,
}

impl Default for RqbitDesktopConfig {
    fn default() -> Self {
        let userdirs = directories::UserDirs::new().expect("directories::UserDirs::new()");
        let download_folder = userdirs
            .download_dir()
            .map(|d| d.to_owned())
            .unwrap_or_else(|| userdirs.home_dir().join("Downloads"));

        Self {
            default_download_location: download_folder,
            dht: Default::default(),
            connections: Default::default(),
            upnp: Default::default(),
            persistence: Default::default(),
            http_api: Default::default(),
            ratelimits: Default::default(),
            #[cfg(feature = "disable-upload")]
            disable_upload: false,
        }
    }
}

impl RqbitDesktopConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        if !self.http_api.disable
            && !self.http_api.read_only
            && !self.http_api.listen_addr.ip().is_loopback()
        {
            anyhow::bail!(
                "the desktop HTTP API must be read-only when listening on a non-loopback address"
            )
        }
        if self.upnp.enable_server {
            if self.http_api.disable {
                anyhow::bail!("if UPnP server is enabled, you need to enable the HTTP API also.")
            }
            if self.http_api.listen_addr.ip().is_loopback() {
                anyhow::bail!(
                    "if UPnP server is enabled, you need to set HTTP API IP to 0.0.0.0 or at least non-localhost address."
                )
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::{Ipv4Addr, SocketAddr, SocketAddrV4},
        path::PathBuf,
        time::Duration,
    };

    use librqbit::{ListenerMode, limits::LimitsConfig};

    use super::{
        RqbitDesktopConfig, RqbitDesktopConfigConnections, RqbitDesktopConfigDht,
        RqbitDesktopConfigHttpApi, RqbitDesktopConfigPersistence, RqbitDesktopConfigUpnp,
    };

    fn config() -> RqbitDesktopConfig {
        RqbitDesktopConfig {
            default_download_location: PathBuf::from("Downloads"),
            dht: RqbitDesktopConfigDht {
                disable: false,
                disable_persistence: false,
                persistence_filename: PathBuf::from("dht.json"),
            },
            connections: RqbitDesktopConfigConnections::default(),
            upnp: RqbitDesktopConfigUpnp::default(),
            persistence: RqbitDesktopConfigPersistence {
                disable: false,
                folder: PathBuf::from("session"),
                fastresume: false,
                filename: PathBuf::new(),
            },
            http_api: RqbitDesktopConfigHttpApi::default(),
            ratelimits: LimitsConfig::default(),
            #[cfg(feature = "disable-upload")]
            disable_upload: false,
        }
    }

    #[test]
    fn writable_http_api_must_remain_on_loopback() {
        let mut config = config();
        config.http_api.listen_addr =
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 3030));

        assert!(config.validate().is_err());

        config.http_api.read_only = true;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn disabled_http_api_may_use_non_loopback_address() {
        let mut config = config();
        config.http_api.disable = true;
        config.http_api.listen_addr = "0.0.0.0:3030".parse().unwrap();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn upnp_server_requires_enabled_non_loopback_http_api() {
        let mut config = config();
        config.upnp.enable_server = true;
        assert!(config.validate().is_err());

        config.http_api.listen_addr = "0.0.0.0:3030".parse().unwrap();
        config.http_api.read_only = true;
        assert!(config.validate().is_ok());

        config.http_api.disable = true;
        assert!(config.validate().is_err());
    }

    #[test]
    fn connection_options_cover_listener_modes_and_proxy() {
        let mut connections = RqbitDesktopConfigConnections {
            enable_tcp_listen: true,
            enable_tcp_outgoing: false,
            enable_utp: true,
            enable_upnp_port_forward: false,
            socks_proxy: "socks5://127.0.0.1:1080".into(),
            listen_port: 6881,
            peer_connect_timeout: Duration::from_secs(3),
            peer_read_write_timeout: Duration::from_secs(4),
        };
        let (listener, outgoing) = connections.as_listener_and_connect_opts();
        let listener = listener.unwrap();
        assert!(matches!(listener.mode, ListenerMode::TcpAndUtp));
        assert_eq!(listener.listen_addr, "0.0.0.0:6881".parse().unwrap());
        assert!(!listener.enable_upnp_port_forwarding);
        assert!(!outgoing.enable_tcp);
        assert_eq!(
            outgoing.proxy_url.as_deref(),
            Some("socks5://127.0.0.1:1080")
        );
        let peer = outgoing.peer_opts.unwrap();
        assert_eq!(peer.connect_timeout, Some(Duration::from_secs(3)));
        assert_eq!(peer.read_write_timeout, Some(Duration::from_secs(4)));

        connections.enable_tcp_listen = false;
        let (listener, _) = connections.as_listener_and_connect_opts();
        assert!(matches!(listener.unwrap().mode, ListenerMode::UtpOnly));
        connections.enable_utp = false;
        assert!(connections.as_listener_and_connect_opts().0.is_none());
    }

    #[test]
    fn persistence_migrates_legacy_filename_only_when_folder_is_empty() {
        let mut persistence = RqbitDesktopConfigPersistence {
            disable: false,
            folder: PathBuf::new(),
            fastresume: false,
            filename: PathBuf::from("legacy/session.json"),
        };
        persistence.fix_backwards_compat();
        assert_eq!(persistence.folder, PathBuf::from("legacy"));

        persistence.folder = PathBuf::from("chosen");
        persistence.filename = PathBuf::from("other/session.json");
        persistence.fix_backwards_compat();
        assert_eq!(persistence.folder, PathBuf::from("chosen"));
    }
}
