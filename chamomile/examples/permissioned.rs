//! Test the permissioned message.
//! Runing 3-node: 1 (relay - S), 2 (stable - A, B)
//! A: `cargo run --example permissioned 127.0.0.1:8000`
//! B: `cargo run --example permissioned 127.0.0.1:8001 127.0.0.1:8000`
//! C: `cargo run --example permissioned 127.0.0.1:8002 127.0.0.1:8000`

use chamomile::prelude::{start, Config, Peer, ReceiveMessage, SendMessage};
use std::{env::args, net::SocketAddr};

#[tokio::main]
async fn main() {
    std::env::set_var("RUST_LOG", "debug");
    tracing_subscriber::fmt::init();

    let addr_str = args().nth(1).expect("missing path");
    let self_addr: SocketAddr = addr_str.parse().expect("invalid addr");

    let mut config = Config::default(Peer::socket(self_addr));
    config.permission = true;
    config.db_dir = std::path::PathBuf::from(format!(".data/permissioned/{}", addr_str));

    let (peer_id, send, mut recv) = start(config).await.unwrap();
    println!("peer id: {}", peer_id.to_hex());

    if args().nth(2).is_some() {
        let remote_addr: SocketAddr = args().nth(2).unwrap().parse().expect("invalid addr");
        println!("start connect to remote: {}", remote_addr);
        send.send(SendMessage::Connect(Peer::socket(remote_addr)))
            .await
            .expect("channel failure!");
    }

    while let Some(message) = recv.recv().await {
        match message {
            ReceiveMessage::Data(peer_id, bytes) => {
                println!("Recv data from: {}, {:?}", peer_id.short_show(), bytes);
            }
            ReceiveMessage::StableConnect(peer, join_data) => {
                println!("Peer join: {:?}, join data: {:?}", peer, join_data);
                send.send(SendMessage::StableResult(0, peer, true, false, vec![1]))
                    .await
                    .expect("channel failure!");
            }
            ReceiveMessage::StableResult(peer, is_ok, data) => {
                println!("Peer Join Result: {:?} {}, data: {:?}", peer, is_ok, data);
            }
            ReceiveMessage::ResultConnect(from, _data) => {
                println!("Recv Result Connect {:?}", from);
            }
            ReceiveMessage::StableLeave(peer) => {
                println!("Peer_leave: {:?}", peer);
            }
            ReceiveMessage::NetworkLost => {
                println!("No peers conneced.")
            }
            _ => {
                panic!("nerver here!");
            }
        }
    }
}
