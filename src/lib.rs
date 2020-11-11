#![recursion_limit = "1024"]
// TODO WIP
#![allow(dead_code)]

//! `chamomile` is a crate for building a solid and efficient p2p network.
//!
//! # Example Use
//!
//! We running a p2p peer, and if others add, we will get the info.
//!
//! ```ignore
//! use async_std::task;
//! use std::env::args;
//! use std::net::SocketAddr;
//!
//! use chamomile::prelude::{start, Config, ReceiveMessage, SendMessage};
//!
//! fn main() {
//!     task::block_on(async {
//!         let self_addr: SocketAddr = args()
//!             .nth(1)
//!             .expect("missing path")
//!             .parse()
//!             .expect("invalid addr");
//!
//!         let (peer_id, send, recv) = start(Config::default(self_addr)).await.unwrap();
//!         println!("peer id: {}", peer_id.short_show());
//!
//!         if args().nth(2).is_some() {
//!             let remote_addr: SocketAddr = args().nth(2).unwrap().parse().expect("invalid addr");
//!             println!("start connect to remote: {}", remote_addr);
//!             send.send(SendMessage::Connect(remote_addr, None))
//!                 .await;
//!         }
//!
//!         while let Some(message) = recv.recv().await {
//!             match message {
//!                 ReceiveMessage::Data(peer_id, bytes) => {
//!                     println!("recv data from: {}, {:?}", peer_id.short_show(), bytes);
//!                 }
//!                 ReceiveMessage::PeerJoin(peer_id, _addr, join_data) => {
//!                     println!("peer join: {:?}, join data: {:?}", peer_id, join_data);
//!                     send.send(SendMessage::PeerJoinResult(peer_id, true, false, vec![1]))
//!                         .await;
//!                     println!("Debug: when join send message test: {:?}", vec![1, 2, 3, 4]);
//!                     send.send(SendMessage::Data(peer_id, vec![1, 2, 3, 4])).await;
//!                 }
//!                 ReceiveMessage::PeerLeave(peer_id) => {
//!                     println!("peer_leave: {:?}", peer_id);
//!                 }
//!             }
//!         }
//!     });
//! }
//! ```
//!
//! # Features
//!
//! - Directly connection & DHT-based connection & Relay connection.
//! - Diff transports: UDP/TCP/UDP-Based Special Protocol.
//! - Support common broadcast.
//!

#[macro_use]
extern crate log;

mod broadcast;
mod config;
mod hole_punching;
mod kad;
mod keys;
mod multicasting;
mod peer;
mod peer_list;
mod server;
mod session;

pub mod primitives;
pub mod transports;

pub mod prelude {
    use smol::{
        channel::{self, Receiver, Sender},
        io::Result,
    };

    pub use chamomile_types::message::{ReceiveMessage, SendMessage, StreamType};
    pub use chamomile_types::types::{Broadcast, PeerId};

    pub use super::config::Config;

    /// new a channel for send message to the chamomile.
    pub fn new_send_channel() -> (Sender<SendMessage>, Receiver<SendMessage>) {
        channel::unbounded()
    }

    /// new a channel for receive the chamomile message.
    pub fn new_receive_channel() -> (Sender<ReceiveMessage>, Receiver<ReceiveMessage>) {
        channel::unbounded()
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
