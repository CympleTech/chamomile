use chamomile::prelude::*;
use chamomile_types::types::TransportType;
use std::{env::args, net::SocketAddr, time::Duration};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_level(true)
        .with_max_level(tracing::Level::DEBUG)
        .init();

    let addr_str = args().nth(1).expect("missing path");
    let self_addr: SocketAddr = addr_str.parse().expect("invalid addr");

    println!("START A PERMISSIONLESS PEER. socket: {}", self_addr);

    let mut peer = Peer::socket(self_addr);
    peer.transport = TransportType::TCP;
    let mut config = Config::default(peer);
    config.permission = false; // Permissionless.
    config.only_stable_data = true; // Only receive stable connected peer's data.
    config.db_dir = std::path::PathBuf::from(addr_str);

    let (peer_id, send, mut recv) = start(config).await.unwrap();
    println!("peer id: {}", peer_id.to_hex());

    if args().nth(2).is_some() {
        let remote_addr: SocketAddr = args().nth(2).unwrap().parse().expect("invalid addr");
        println!("start DHT connect to remote: {}", remote_addr);
        let mut relay = Peer::socket(remote_addr);
        relay.transport = TransportType::TCP;
        let _ = send.send(SendMessage::Connect(relay)).await;

        if args().nth(3).is_some() {
            println!("sleep 3s and then start stable connection...");
            tokio::time::sleep(Duration::from_secs(2)).await;
            let peer_id_str = args().nth(3).unwrap();
            let peer_id = PeerId::from_hex(peer_id_str).unwrap(); // test peer_id

            let mut bytes = vec![];
            for i in 0..10u8 {
                bytes.push(i);
            }
            let _ = send
                .send(SendMessage::StableConnect(1, Peer::peer(peer_id), bytes))
                .await;
        }
    }

    let mut first_data = true;

    while let Some(message) = recv.recv().await {
        match message {
            ReceiveMessage::Data(peer_id, bytes) => {
                println!(
                    "==========Recv permissioned data from: {}, {}-{:?}, start build a stable connection",
                    peer_id.short_show(),
                    bytes.len(),
                    bytes
                );

                if first_data {
                    send.send(SendMessage::Data(2, peer_id, bytes))
                        .await
                        .unwrap();
                    first_data = false;

                    if args().nth(3).is_some() {
                        println!("=========== START DIRECTLY INCOMING TEST=============");
                        // upgrade to directly
                        send.send(SendMessage::Connect(Peer::socket(
                            "127.0.0.1:7365".parse().unwrap(),
                        )))
                        .await
                        .unwrap();
                    }
                }
            }
            ReceiveMessage::StableConnect(from, data) => {
                println!(
                    "==========Recv peer what to build a stable connected: {:?}",
                    data
                );

                let _ = send
                    .send(SendMessage::StableResult(
                        3,
                        from,
                        true,
                        false,
                        vec![3, 3, 3, 3],
                    ))
                    .await;
            }
            ReceiveMessage::StableLeave(peer) => {
                println!("===========Recv stable connected leave: {:?}", peer);
            }
            ReceiveMessage::StableResult(peer, is_ok, remark) => {
                println!(
                    "=========Recv stable connected result: {:?} {} {:?}",
                    peer, is_ok, remark
                );

                send.send(SendMessage::Data(4, peer_id, vec![1, 2, 3, 4, 5]))
                    .await
                    .unwrap();
            }
            ReceiveMessage::ResultConnect(from, _data) => {
                println!("Recv Result Connect {:?}", from);
            }
            ReceiveMessage::Delivery(t, tid, had, _data) => {
                println!("======== ===== Recv {:?} Delivery: {} {}", t, tid, had);
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
