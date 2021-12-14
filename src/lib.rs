#![recursion_limit = "1024"]

//! `chamomile` is a crate for building a solid and efficient p2p network.
//!
//! # Example Use
//!
//! We running a p2p peer, and if others add, we will get the info.
//!
//! ```ignore
//! use std::env::args;
//! use std::net::SocketAddr;
//! use std::path::PathBuf;
//!
//! use chamomile::prelude::{start, Config, Peer, ReceiveMessage, SendMessage};
//!
//! #[tokio::main]
//! async fn main() {
//!    let addr_str = args().nth(1).expect("missing path");
//!    let self_addr: SocketAddr = addr_str.parse().expect("invalid addr");
//!
//!    println!("START A INDEPENDENT P2P RELAY SERVER. : {}", self_addr);
//!
//!    let mut config = Config::default(Peer::socket(self_addr));
//!    config.permission = false;
//!    config.only_stable_data = true;
//!    config.db_dir = std::path::PathBuf::from("./");
//!
//!    let (peer_id, send, mut recv) = start(config).await.unwrap();
//!    println!("peer id: {}", peer_id.to_hex());
//!
//!    if args().nth(2).is_some() {
//!        let remote_addr: SocketAddr = args().nth(2).unwrap().parse().expect("invalid addr");
//!        println!("start DHT connect to remote: {}", remote_addr);
//!        send.send(SendMessage::Connect(Peer::socket(remote_addr)))
//!            .await
//!            .expect("channel failure");
//!    }
//!
//!    while let Some(message) = recv.recv().await {
//!        match message {
//!            ReceiveMessage::Data(..) => {}
//!            ReceiveMessage::Stream(..) => {}
//!            ReceiveMessage::StableConnect(from, ..) => {
//!                let _ = send
//!                    .send(SendMessage::StableResult(0, from, false, false, vec![]))
//!                    .await;
//!            }
//!            ReceiveMessage::ResultConnect(from, ..) => {
//!                let _ = send
//!                    .send(SendMessage::StableResult(0, from, false, false, vec![]))
//!                    .await;
//!            }
//!            ReceiveMessage::StableLeave(..) => {}
//!            ReceiveMessage::StableResult(..) => {}
//!            ReceiveMessage::Delivery(..) => {}
//!            ReceiveMessage::NetworkLost => {}
//!        }
//!    }
//! }
//! ```
//!
//! # Features
//!
//! - Support build a robust stable connection between two peers on the p2p network.
//! - Support permissionless network.
//! - Support permissioned network (distributed network).
//! - DHT-based & Relay connection.
//! - Diff transports: QUIC(*default*) / TCP / UDP-Based Special Protocol.

#[macro_use]
extern crate log;

mod buffer;
mod config;
mod global;
mod hole_punching;
mod kad;
mod keys;
mod lan;
mod peer_list;
mod server;
mod session;

pub mod primitives;
pub mod transports;

pub mod prelude {
    pub use chamomile_types::message::{
        DeliveryType, ReceiveMessage, SendMessage, StateRequest, StateResponse, StreamType,
    };
    pub use chamomile_types::types::{Broadcast, PeerId};
    pub use chamomile_types::Peer;

    use tokio::{
        io::Result,
        sync::mpsc::{self, Receiver, Sender},
    };

    pub use super::config::Config;

    /// new a channel for send message to the chamomile.
    pub fn new_send_channel() -> (Sender<SendMessage>, Receiver<SendMessage>) {
        mpsc::channel(128)
    }

    /// new a channel for receive the chamomile message.
    pub fn new_receive_channel() -> (Sender<ReceiveMessage>, Receiver<ReceiveMessage>) {
        mpsc::channel(128)
    }

    /// main function. start a p2p service.
    pub async fn start(
        config: Config,
    ) -> Result<(PeerId, Sender<SendMessage>, Receiver<ReceiveMessage>)> {
        info!("start p2p service...");
        let (send_send, send_recv) = new_send_channel();
        let (recv_send, recv_recv) = new_receive_channel();

        let peer_id = super::server::start(config, recv_send, send_recv).await?;
        info!("start p2p ok.");

        Ok((peer_id, send_send, recv_recv))
    }
}
