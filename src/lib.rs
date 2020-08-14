#![recursion_limit = "1024"]
// TODO Debug
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]

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
mod keys;
mod message;
mod multicasting;
mod peer;
mod peer_list;
mod server;
mod session;
mod storage;

pub mod primitives;
pub mod transports;

pub mod prelude {
    use async_std::{
        io::Result,
        sync::{channel, Receiver, Sender},
    };

    use super::primitives::MAX_MESSAGE_CAPACITY;

    pub use super::broadcast::Broadcast;
    pub use super::config::Config;
    pub use super::message::{ReceiveMessage, SendMessage, StreamType};
    pub use super::peer::PeerId;
    pub use super::storage::LocalDB;

    /// new a channel for send message to the chamomile.
    pub fn new_send_channel() -> (Sender<SendMessage>, Receiver<SendMessage>) {
        channel::<SendMessage>(MAX_MESSAGE_CAPACITY)
    }

    /// new a channel for receive the chamomile message.
    pub fn new_receive_channel() -> (Sender<ReceiveMessage>, Receiver<ReceiveMessage>) {
        channel::<ReceiveMessage>(MAX_MESSAGE_CAPACITY)
    }

    /// main function. start a p2p service.
    pub async fn start(
        config: Config,
    ) -> Result<(PeerId, Sender<SendMessage>, Receiver<ReceiveMessage>)> {
        let (send_send, send_recv) = new_send_channel();
        let (recv_send, recv_recv) = new_receive_channel();

        let peer_id = super::server::start(config, recv_send, send_recv).await?;

        Ok((peer_id, send_send, recv_recv))
    }
}
