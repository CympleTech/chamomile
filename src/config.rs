use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use chamomile_types::types::PeerId;

/// Chammomile Configs.
#[derive(Debug, Clone)]
pub struct Config {
    /// Default Data saved directory.
    pub db_dir: PathBuf,
    /// Default binding SocketAddr.
    pub addr: SocketAddr,
    /// Default transport type.
    pub transport: String,
    /// Allowed Ip's list.
    pub white_list: Vec<SocketAddr>,
    /// Blocked Ip's list.
    pub black_list: Vec<IpAddr>,
    /// Allowed peer's `PeerId` list.
    pub white_peer_list: Vec<PeerId>,
    /// Blocked peers's `PeerId` list.
    pub black_peer_list: Vec<PeerId>,
    /// If set permission is true, that server is permissioned,
    /// not receive DHT's peer message, only stable connect.
    /// if set permissionless is false, that server is permissionless,
    /// receive DHT's peer message and stable's peer message.
    /// if you use a permissionless server, but only receive stable's message,
    /// you can set `only_stable_data` is true.
    /// Recommend use `permission = false & only_stable_data = true` replace permissioned.
    pub permission: bool,
    /// If `only_stable_data` is true, only receive stable connected peer's data.
    pub only_stable_data: bool,
}

impl Config {
    pub fn default(addr: SocketAddr) -> Self {
        Self {
            db_dir: PathBuf::from("./"),
            addr: addr,
            transport: "tcp".to_owned(), // TODO Default
            white_list: vec![],
            black_list: vec![],
            white_peer_list: vec![],
            black_peer_list: vec![],
            permission: false,
            only_stable_data: false,
        }
    }

    pub fn new(
        db_dir: PathBuf,
        addr: SocketAddr,
        transport: String,
        white_list: Vec<SocketAddr>,
        black_list: Vec<IpAddr>,
        white_peer_list: Vec<PeerId>,
        black_peer_list: Vec<PeerId>,
        permission: bool,
        only_stable_data: bool,
    ) -> Self {
        Self {
            db_dir,
            addr,
            transport,
            white_list,
            black_list,
            white_peer_list,
            black_peer_list,
            permission,
            only_stable_data,
        }
    }
}
