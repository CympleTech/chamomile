//! Test the own (same PeerId) message.
//! Runing 3-node: 1 (relay - S), 2 (own - A, B)
//! 1. Run S at 8000: `cargo run --example relay 192.168.xx.xx:8000`
//! 2. Run A at 127.0.0.1:0: `cargo run --example own 192.168.xx.xx:8000`
//! 3. Run B at 127.0.0.1:0: `cargo run --example own 192.168.xx.xx:8000`

use chamomile::prelude::*;
use chamomile_types::{key::secp256k1::rand::Rng, types::TransportType};
use rand_chacha::{rand_core::SeedableRng, ChaChaRng};
use std::{env::args, net::SocketAddr};

#[tokio::main]
async fn main() {
    std::env::set_var("RUST_LOG", "debug");
    tracing_subscriber::fmt()
        .with_level(true)
        .with_max_level(tracing::Level::DEBUG)
        .init();

    let addr_str = args().nth(1).expect("missing realy");
    let relay: SocketAddr = addr_str.parse().expect("invalid addr");

    let mut rng = ChaChaRng::from_entropy();
    let mut peer = Peer::socket("127.0.0.1:0".parse().unwrap());
    peer.transport = TransportType::TCP;
    let mut config = Config::default(peer);
    config.permission = true;
    config.db_dir = std::path::PathBuf::from(format!(".data/own/{}", rng.gen::<u8>()));

    // default key to test own.
    let key = Key::from_db_bytes(&[
        0, 72, 137, 44, 19, 236, 242, 211, 157, 163, 190, 217, 116, 14, 149, 254, 211, 242, 248,
        101, 191, 114, 185, 88, 249, 177, 115, 181, 251, 9, 10, 71, 13, 236, 8, 166, 64, 201, 101,
        183, 186, 156, 138, 166, 75, 253, 158, 211, 124, 155, 152, 89, 33, 8, 72, 160, 108, 248,
        205, 76, 100, 75, 133, 247, 202,
    ])
    .unwrap();

    let (peer_id, send, mut recv) = start_with_key(config, key).await.unwrap();
    println!("peer id: {}", peer_id.to_hex());
    let mut relay = Peer::socket(relay);
    relay.transport = TransportType::QUIC;
    let _ = send.send(SendMessage::Connect(relay)).await;

    while let Some(message) = recv.recv().await {
        match message {
            ReceiveMessage::OwnConnect(peer) => {
                println!("Own connected, assist: {}", peer.assist.to_hex());
                let _ = send.send(SendMessage::OwnEvent(vec![1, 2, 3, 4])).await;
            }
            ReceiveMessage::OwnEvent(_pid, data) => {
                println!("Receive data: {:?}", data);
            }
            ReceiveMessage::OwnLeave(peer) => {
                println!("Own leaved, assist: {}", peer.assist.to_hex());
            }
            ReceiveMessage::NetworkLost => {
                println!("Network lost...");
            }
            _ => {
                panic!("Nerver here!!!")
            }
        }
    }
}
