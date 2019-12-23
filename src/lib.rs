use async_std::{
    io::Result,
    sync::{channel, Receiver, Sender},
};
use std::net::SocketAddr;

mod core;
mod transports;

pub const MAX_MESSAGE_CAPACITY: usize = 1024;

#[derive(Debug, Clone, Default, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct PeerId;

#[derive(Debug)]
pub enum Message {
    PeerJoin(PeerId),
    PeerLeave(PeerId),
    Connect(SocketAddr),
    DisConnect(SocketAddr),
    Data(Vec<u8>, PeerId),
}

#[derive(Debug, Clone)]
pub struct Config {
    pub addr: SocketAddr,
    pub white_list: Vec<SocketAddr>,
    pub black_list: Vec<SocketAddr>,
    pub white_peer_list: Vec<PeerId>,
    pub black_peer_list: Vec<PeerId>,
}

impl Config {
    pub fn default(addr: SocketAddr) -> Self {
        Self {
            addr: addr,
            white_list: vec![],
            black_list: vec![],
            white_peer_list: vec![],
            black_peer_list: vec![],
        }
    }

    pub fn new(
        addr: SocketAddr,
        white_list: Vec<SocketAddr>,
        black_list: Vec<SocketAddr>,
        white_peer_list: Vec<PeerId>,
        black_peer_list: Vec<PeerId>,
    ) -> Self {
        Self {
            addr,
            white_list,
            black_list,
            white_peer_list,
            black_peer_list,
        }
    }
}

pub fn new_channel() -> (Sender<Message>, Receiver<Message>) {
    channel::<Message>(MAX_MESSAGE_CAPACITY)
}

pub async fn start(out_send: Sender<Message>, config: Config) -> Result<Sender<Message>> {
    let (send, recv) = new_channel();

    core::server::Server::start(core::server::Server::new(config), out_send, recv).await?;

    Ok(send)
}
