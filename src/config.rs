use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use chamomile_types::types::PeerId;

#[derive(Debug, Clone)]
pub struct Config {
    pub db_dir: PathBuf,
    pub addr: SocketAddr,
    pub join_data: Vec<u8>,
    pub transport: String,
    pub white_list: Vec<SocketAddr>,
    pub black_list: Vec<IpAddr>,
    pub white_peer_list: Vec<PeerId>,
    pub black_peer_list: Vec<PeerId>,
    pub permission: bool,
}

impl Config {
    pub fn default(addr: SocketAddr) -> Self {
        Self {
            db_dir: PathBuf::from("./"),
            addr: addr,
            join_data: vec![],
            transport: "tcp".to_owned(), // TODO Default
            white_list: vec![],
            black_list: vec![],
            white_peer_list: vec![],
            black_peer_list: vec![],
            permission: false,
        }
    }

    pub fn new(
        db_dir: PathBuf,
        addr: SocketAddr,
        join_data: Vec<u8>,
        transport: String,
        white_list: Vec<SocketAddr>,
        black_list: Vec<IpAddr>,
        white_peer_list: Vec<PeerId>,
        black_peer_list: Vec<PeerId>,
        permission: bool,
    ) -> Self {
        Self {
            db_dir,
            addr,
            join_data,
            transport,
            white_list,
            black_list,
            white_peer_list,
            black_peer_list,
            permission,
        }
    }
}
