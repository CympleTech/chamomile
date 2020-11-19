use simplelog::{CombinedLogger, Config as LogConfig, LevelFilter, TermLogger, TerminalMode};
use std::env::args;
use std::net::SocketAddr;

use chamomile::prelude::{start, Config, ReceiveMessage, SendMessage};
use chamomile_types::types::PeerId;
use smol::Timer;
use std::time::Duration;

fn main() {
    CombinedLogger::init(vec![TermLogger::new(
        LevelFilter::Debug,
        LogConfig::default(),
        TerminalMode::Mixed,
    )])
    .unwrap();

    smol::block_on(async {
        let addr_str = args().nth(1).expect("missing path");
        let self_addr: SocketAddr = addr_str.parse().expect("invalid addr");

        println!("START A PERMISSIONLESS PEER. socket: {}", self_addr);

        let mut config = Config::default(self_addr);
        config.permission = false; // Permissionless.
        config.only_stable_data = true; // Only receive stable connected peer's data.
        config.db_dir = std::path::PathBuf::from(addr_str);

        let (peer_id, send, recv) = start(config).await.unwrap();
        println!("peer id: {}", peer_id.to_hex());

        if args().nth(2).is_some() {
            let remote_addr: SocketAddr = args().nth(2).unwrap().parse().expect("invalid addr");
            println!("start DHT connect to remote: {}", remote_addr);
            send.send(SendMessage::Connect(remote_addr))
                .await
                .expect("channel failure");

            if args().nth(3).is_some() {
                println!("sleep 3s and then start stable connection...");
                Timer::after(Duration::from_secs(2)).await;
                let peer_id_str = args().nth(3).unwrap();
                let peer_id = PeerId::from_hex(peer_id_str).unwrap(); // test peer_id

                let mut bytes = vec![];
                for i in 0..10u8 {
                    bytes.push(i);
                }
                send.send(SendMessage::StableConnect(
                    1,
                    peer_id,
                    Some("127.0.0.1:7367".parse().unwrap()),
                    bytes,
                ))
                .await
                .expect("channel failure");
            }
        }

        let mut first_data = true;

        while let Ok(message) = recv.recv().await {
            match message {
                ReceiveMessage::Data(peer_id, bytes) => {
                    println!(
                        "==========Recv permissioned data from: {}, {}-{:?}, start build a stable connection",
                        peer_id.short_show(),
                        bytes.len(),
                        bytes
                    );

                    if first_data {
                        let _ = send.send(SendMessage::Data(2, peer_id, bytes)).await;
                        first_data = false;
                    }
                }
                ReceiveMessage::Stream(..) => {
                    panic!("Nerver here (stream)");
                }
                ReceiveMessage::StableConnect(from, data) => {
                    println!(
                        "==========Recv peer what to build a stable connected: {:?}",
                        data
                    );

                    send.send(SendMessage::StableResult(
                        3,
                        from,
                        true,
                        false,
                        vec![3, 3, 3, 3],
                    ))
                    .await
                    .expect("channel failure");
                }
                ReceiveMessage::StableLeave(peer_id) => {
                    println!(
                        "===========Recv stable connected leave: {}",
                        peer_id.to_hex()
                    );
                }
                ReceiveMessage::StableResult(peer_id, is_ok, remark) => {
                    println!(
                        "=========Recv stable connected result: {} {} {:?}",
                        peer_id.to_hex(),
                        is_ok,
                        remark
                    );

                    let _ = send
                        .send(SendMessage::Data(4, peer_id, vec![1, 2, 3, 4, 5]))
                        .await;
                }
                ReceiveMessage::Delivery(t, tid, had) => {
                    println!("======== ===== Recv {:?} Delivery: {} {}", t, tid, had);
                }
            }
        }
    });
}
