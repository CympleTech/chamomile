use async_std::task;
use std::env::args;
use std::net::SocketAddr;

use chamomile::prelude::{start, Config, ReceiveMessage, SendMessage};

fn main() {
    task::block_on(async {
        let self_addr: SocketAddr = args()
            .nth(1)
            .expect("missing path")
            .parse()
            .expect("invalid addr");

        let (peer_id, send, recv) = start(Config::default(self_addr)).await.unwrap();
        println!("peer id: {}", peer_id.to_hex());

        if args().nth(2).is_some() {
            let remote_addr: SocketAddr = args().nth(2).unwrap().parse().expect("invalid addr");
            println!("start connect to remote: {}", remote_addr);
            send.send(SendMessage::Connect(remote_addr, None)).await;
        }

        while let Ok(message) = recv.recv().await {
            match message {
                ReceiveMessage::Data(peer_id, bytes) => {
                    println!("recv data from: {}, {:?}", peer_id.short_show(), bytes);
                }
                ReceiveMessage::Stream(..) => {
                    panic!("Not stream");
                }
                ReceiveMessage::PeerJoin(..) => {
                    panic!("Default is permissionless and not PeerConnect");
                }
                ReceiveMessage::PeerLeave(..) => {
                    panic!("Default is permissionless and not PeerConnect");
                }
                ReceiveMessage::PeerJoinResult(..) => {
                    panic!("Default is permissionless and not PeerConnect");
                }
            }
        }
    });
}
