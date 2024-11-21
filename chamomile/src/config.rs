use std::net::IpAddr;
use std::path::PathBuf;

use chamomile_types::{Peer, PeerId};

/// Chammomile Configs.
#[derive(Debug, Clone)]
pub struct Config {
    /// Default Data saved directory.
    pub db_dir: PathBuf,
    /// Default binding multiaddr string.
    /// Example: "/ip4/0.0.0.0/quic/7364"
    pub peer: Peer,
    /// Allowed MultiAddr style peer list.
    pub allowlist: Vec<Peer>,
    /// Blocked Ip's list.
    pub blocklist: Vec<IpAddr>,
    /// Allowed peer's `PeerId` list.
    pub allow_peer_list: Vec<PeerId>,
    /// Blocked peers's `PeerId` list.
    pub block_peer_list: Vec<PeerId>,
    /// If set permission is true, that server is permissioned,
    /// not receive DHT's peer message, only stable connect.
    /// if set permission is false, that server is permissionless,
    /// receive DHT's peer message and stable's peer message.
    /// if you use a permissionless server, but only receive stable's message,
    /// you can set `only_stable_data` is true.
    /// Recommend use `permission = false & only_stable_data = true` replace permissioned.
    pub permission: bool,
    /// If `only_stable_data` is true, only receive stable connected peer's data.
    pub only_stable_data: bool,
    /// When delivery feedback has set length, it will split length of data to return.
    /// For example. set `delivery_length = 8`,
    /// and when a `Data(1u64, PeerId, vec![1u8, 2u8, ..., 100u8]),
    /// if send success, will return:
    /// `Delivery(DeliveryType::Data, 1u64, true, vec![1u8, 2u8, ..., 8u8])`
    /// if send failure, will return:
    /// `Delivery(DeliveryType::Data, 1u64, false, vec![1u8, 2u8, ..., 8u8])`
    pub delivery_length: usize,
}

impl Config {
    pub fn default(peer: Peer) -> Self {
        Self {
            db_dir: PathBuf::from("./"),
            peer: peer,
            allowlist: vec![],
            blocklist: vec![],
            allow_peer_list: vec![],
            block_peer_list: vec![],
            permission: false,
            only_stable_data: false,
            delivery_length: 0,
        }
    }

    pub fn new(
        db_dir: PathBuf,
        peer: Peer,
        allowlist: Vec<Peer>,
        blocklist: Vec<IpAddr>,
        allow_peer_list: Vec<PeerId>,
        block_peer_list: Vec<PeerId>,
        permission: bool,
        only_stable_data: bool,
        delivery_length: usize,
    ) -> Self {
        Self {
            db_dir,
            peer,
            allowlist,
            blocklist,
            allow_peer_list,
            block_peer_list,
            permission,
            only_stable_data,
            delivery_length,
        }
    }
}
