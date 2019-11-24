use std::net::SocketAddr;
use async_std::sync::{channel, Sender, Receiver};
use async_std::io::Result;
use async_std::task;

mod server;
mod transports;

use server::Server;

pub const MAX_MESSAGE_CAPACITY: usize = 1024;

#[derive(Debug, Clone)]
pub struct PeerId;

#[derive(Debug)]
pub enum Message {
    PeerJoin(PeerId),
    PeerLeave(PeerId),
    Connect(SocketAddr),
    DisConnect(SocketAddr),
    Data(Vec<u8>),
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
            black_peer_list: vec![]
        }
    }

    pub fn new(
        addr: SocketAddr,
        white_list: Vec<SocketAddr>,
        black_list: Vec<SocketAddr>,
        white_peer_list: Vec<PeerId>,
        black_peer_list: Vec<PeerId>
    ) -> Self {
        Self { addr, white_list, black_list, white_peer_list, black_peer_list }
    }
}

pub fn new_channel() -> (Sender<Message>, Receiver<Message>) {
    channel::<Message>(MAX_MESSAGE_CAPACITY)
}

pub async fn start(out_send: Sender<Message>, config: Config) -> Result<Sender<Message>> {
    let (send, recv) = new_channel();

    task::spawn(async {
        let server = Server::new(out_send, recv, config);
        server.start().await
    });

    Ok(send)
}
