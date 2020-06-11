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
            send.send(SendMessage::Connect(remote_addr, Some(vec![1])))
                .await;
        }

        while let Ok(message) = recv.recv().await {
            match message {
                ReceiveMessage::Data(peer_id, bytes) => {
                    println!("recv data from: {}, {:?}", peer_id.short_show(), bytes);
                }
                ReceiveMessage::PeerJoin(peer_id, addr, join_data) => {
                    println!(
                        "peer join: {:?} {}, join data: {:?}",
                        peer_id, addr, join_data
                    );
                    send.send(SendMessage::PeerJoinResult(peer_id, true, false, vec![1]))
                        .await;

                    // Test Relay
                    // send.send(SendMessage::Data(
                    //     PeerId::from_hex(
                    //         "63d6dca953e4941dc2a02298cf8964a26fa4b30659d66b7b90c1dd094d46ea05",
                    //     )
                    //     .unwrap(),
                    //     vec![1, 1, 1, 1, 1],
                    // ))
                    // .await;
                }
                ReceiveMessage::PeerLeave(peer_id) => {
                    println!("peer_leave: {:?}", peer_id);
                }
                _ => {}
            }
        }
    });
}
