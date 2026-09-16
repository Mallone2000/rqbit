use std::{collections::VecDeque, net::SocketAddr, str::FromStr, sync::atomic::AtomicU32};

use bencode::ByteBufOwned;
use chrono::{DateTime, Utc};
use librqbit_core::{compact_ip::CompactSocketAddr, hash_id::Id20};
use parking_lot::RwLock;
use rand::Rng;
use serde::{
    Deserialize, Serialize,
    ser::{SerializeMap, SerializeStruct},
};
use tracing::trace;

use crate::bprotocol::{AnnouncePeer, Want};

#[derive(Serialize, Deserialize)]
struct StoredToken {
    token: [u8; 4],
    #[serde(serialize_with = "crate::utils::serialize_id20")]
    node_id: Id20,
    addr: SocketAddr,
}

#[derive(Serialize, Deserialize)]
struct StoredPeer {
    addr: SocketAddr,
    time: DateTime<Utc>,
}

pub struct PeerStore {
    self_id: Id20,
    max_remembered_tokens: u32,
    max_remembered_peers: u32,
    max_distance: Id20,
    tokens: RwLock<VecDeque<StoredToken>>,
    peers: dashmap::DashMap<Id20, Vec<StoredPeer>>,
    peers_len: AtomicU32,
}

impl Serialize for PeerStore {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        struct SerializePeers<'a> {
            peers: &'a dashmap::DashMap<Id20, Vec<StoredPeer>>,
        }

        impl Serialize for SerializePeers<'_> {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                let mut m = serializer.serialize_map(None)?;
                for entry in self.peers.iter() {
                    m.serialize_entry(&entry.key().as_string(), &entry.value())?;
                }
                m.end()
            }
        }

        let mut s = serializer.serialize_struct("PeerStore", 7)?;
        s.serialize_field("self_id", &self.self_id.as_string())?;
        s.serialize_field("max_remembered_tokens", &self.max_remembered_tokens)?;
        s.serialize_field("max_remembered_peers", &self.max_remembered_peers)?;
        s.serialize_field("max_distance", &self.max_distance.as_string())?;
        s.serialize_field("tokens", &*self.tokens.read())?;
        s.serialize_field("peers", &SerializePeers { peers: &self.peers })?;
        s.serialize_field(
            "peers_len",
            &self.peers_len.load(std::sync::atomic::Ordering::SeqCst),
        )?;
        s.end()
    }
}

impl<'de> Deserialize<'de> for PeerStore {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Tmp {
            self_id: Id20,
            max_remembered_tokens: u32,
            max_remembered_peers: u32,
            max_distance: Id20,
            tokens: VecDeque<StoredToken>,
            peers: dashmap::DashMap<Id20, Vec<StoredPeer>>,
        }

        Tmp::deserialize(deserializer).map(|tmp| Self {
            self_id: tmp.self_id,
            max_remembered_tokens: tmp.max_remembered_tokens,
            max_remembered_peers: tmp.max_remembered_peers,
            max_distance: tmp.max_distance,
            tokens: RwLock::new(tmp.tokens),
            peers_len: AtomicU32::new(tmp.peers.iter().map(|e| e.value().len() as u32).sum()),
            peers: tmp.peers,
        })
    }
}

impl PeerStore {
    pub fn new(self_id: Id20) -> Self {
        Self {
            self_id,
            max_remembered_tokens: 1000,
            max_remembered_peers: 1000,
            max_distance: Id20::from_str("00000fffffffffffffffffffffffffffffffffff").unwrap(),
            tokens: RwLock::new(VecDeque::new()),
            peers: dashmap::DashMap::new(),
            peers_len: AtomicU32::new(0),
        }
    }

    pub fn gen_token_for(&self, node_id: Id20, addr: SocketAddr) -> [u8; 4] {
        let mut token = [0u8; 4];
        rand::rng().fill_bytes(&mut token);
        let mut tokens = self.tokens.write();
        tokens.push_back(StoredToken {
            token,
            addr,
            node_id,
        });
        if tokens.len() > self.max_remembered_tokens as usize {
            tokens.pop_front();
        }
        token
    }

    pub fn store_peer(&self, announce: &AnnouncePeer<ByteBufOwned>, mut addr: SocketAddr) -> bool {
        // If the info_hash in announce is too far away from us, don't store it.
        // If the token doesn't match, don't store it.
        // If we are out of capacity, don't store it.
        // Otherwise, store it.
        if announce.info_hash.distance(&self.self_id) > self.max_distance {
            trace!("peer store: info_hash too far to store");
            return false;
        }
        if !self.tokens.read().iter().any(|t| {
            t.token[..] == announce.token.as_ref()[..] && t.addr == addr && t.node_id == announce.id
        }) {
            trace!("peer store: can't find this token / addr combination");
            return false;
        }

        if announce.implied_port == 0 {
            addr.set_port(announce.port);
        }

        use dashmap::mapref::entry::Entry;
        let peers_entry = self.peers.entry(announce.info_hash);
        let peers_len = self.peers_len.load(std::sync::atomic::Ordering::SeqCst);
        match peers_entry {
            Entry::Occupied(mut occ) => {
                if let Some(s) = occ.get_mut().iter_mut().find(|s| s.addr == addr) {
                    s.time = Utc::now();
                    return true;
                }
                if peers_len >= self.max_remembered_peers {
                    trace!("peer store: out of capacity");
                    return false;
                }
                occ.get_mut().push(StoredPeer {
                    addr,
                    time: Utc::now(),
                });
            }
            Entry::Vacant(vac) => {
                if peers_len >= self.max_remembered_peers {
                    trace!("peer store: out of capacity");
                    return false;
                }
                vac.insert(vec![StoredPeer {
                    addr,
                    time: Utc::now(),
                }]);
            }
        }

        self.peers_len
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        true
    }

    pub fn get_for_info_hash(&self, info_hash: Id20, want: Want) -> Vec<CompactSocketAddr> {
        if let Some(stored_peers) = self.peers.get(&info_hash) {
            return stored_peers
                .iter()
                .filter(|p| {
                    matches!(
                        (p.addr, want),
                        (SocketAddr::V6(..), Want::V6 | Want::Both)
                            | (SocketAddr::V4(..), Want::V4 | Want::Both)
                    )
                })
                .map(|p| p.addr.into())
                .collect();
        }
        Vec::new()
    }

    #[allow(dead_code)]
    pub fn garbage_collect_peers(&self) {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use bencode::ByteBufOwned;
    use librqbit_core::Id20;

    use crate::bprotocol::{AnnouncePeer, Want};

    use super::PeerStore;

    fn id(byte: u8) -> Id20 {
        Id20::new([byte; 20])
    }

    fn announce(
        node_id: Id20,
        info_hash: Id20,
        token: [u8; 4],
        port: u16,
        implied_port: u8,
    ) -> AnnouncePeer<ByteBufOwned> {
        AnnouncePeer {
            id: node_id,
            implied_port,
            info_hash,
            port,
            token: token.to_vec().into(),
        }
    }

    #[test]
    fn valid_token_stores_peer_and_filters_by_address_family() {
        let store = PeerStore::new(Id20::default());
        let node = id(1);
        let hash = Id20::default();
        let source: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        let token = store.gen_token_for(node, source);

        assert!(store.store_peer(&announce(node, hash, token, 6881, 0), source));
        assert_eq!(store.get_for_info_hash(hash, Want::V4).len(), 1);
        assert_eq!(store.get_for_info_hash(hash, Want::Both).len(), 1);
        assert!(store.get_for_info_hash(hash, Want::V6).is_empty());
        assert!(store.get_for_info_hash(hash, Want::None).is_empty());
    }

    #[test]
    fn implied_port_uses_source_port() {
        let store = PeerStore::new(Id20::default());
        let node = id(2);
        let hash = Id20::default();
        let source: SocketAddr = "[::1]:5000".parse().unwrap();
        let token = store.gen_token_for(node, source);
        assert!(store.store_peer(&announce(node, hash, token, 6881, 1), source));

        let peers = store.get_for_info_hash(hash, Want::V6);
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].0, source);
    }

    #[test]
    fn rejects_unknown_token_wrong_source_and_wrong_node() {
        let store = PeerStore::new(Id20::default());
        let node = id(3);
        let hash = Id20::default();
        let source: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        let token = store.gen_token_for(node, source);

        assert!(!store.store_peer(&announce(node, hash, [0; 4], 1, 0), source));
        assert!(!store.store_peer(
            &announce(node, hash, token, 1, 0),
            "127.0.0.1:5001".parse().unwrap()
        ));
        assert!(!store.store_peer(&announce(id(4), hash, token, 1, 0), source));
        assert!(store.get_for_info_hash(hash, Want::Both).is_empty());
    }

    #[test]
    fn duplicate_announce_refreshes_without_duplicate_peer() {
        let store = PeerStore::new(Id20::default());
        let node = id(5);
        let hash = Id20::default();
        let source: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        let token = store.gen_token_for(node, source);
        let announce = announce(node, hash, token, 6881, 0);
        assert!(store.store_peer(&announce, source));
        assert!(store.store_peer(&announce, source));
        assert_eq!(store.get_for_info_hash(hash, Want::Both).len(), 1);
    }

    #[test]
    fn serialization_round_trip_preserves_peers() {
        let store = PeerStore::new(Id20::default());
        let node = id(6);
        let hash = Id20::default();
        let source: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        let token = store.gen_token_for(node, source);
        assert!(store.store_peer(&announce(node, hash, token, 6881, 0), source));

        let encoded = serde_json::to_string(&store).unwrap();
        let decoded: PeerStore = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.get_for_info_hash(hash, Want::Both).len(), 1);
    }
}
