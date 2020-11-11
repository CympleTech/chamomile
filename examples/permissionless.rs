use simplelog::{CombinedLogger, Config as LogConfig, LevelFilter, TermLogger, TerminalMode};
use std::env::args;
use std::net::SocketAddr;

use chamomile::prelude::{start, Config, ReceiveMessage, SendMessage};
use chamomile_types::types::{Broadcast, PeerId};
use smol::Timer;
use std::time::Duration;

fn main() {
    CombinedLogger::init(vec![TermLogger::new(
        LevelFilter::Debug,
        LogConfig::default(),
        TerminalMode::Mixed,
    )
    .unwrap()])
    .unwrap();

    smol::block_on(async {
        let addr_str = args().nth(1).expect("missing path");
        let self_addr: SocketAddr = addr_str.parse().expect("invalid addr");

        println!("START A PERMISSIONLESS PEER. socket: {}", self_addr);

        let mut config = Config::default(self_addr);
        config.permission = false; // Permissionless.
        config.only_stable_data = false; // Receive all peer's data.
        config.db_dir = std::path::PathBuf::from(addr_str);

        let (peer_id, send, recv) = start(config).await.unwrap();
        println!("peer id: {}", peer_id.to_hex());

        if args().nth(2).is_some() {
            let remote_addr: SocketAddr = args().nth(2).unwrap().parse().expect("invalid addr");
            println!("start DHT connect to remote: {}", remote_addr);
            send.send(SendMessage::Connect(remote_addr))
                .await
                .expect("channel failure");

            Timer::after(Duration::from_secs(3)).await;

            fn mod_reduce(mut i: u32) -> u8 {
                loop {
                    if i > 255 {
                        i = i - 255
                    } else {
                        break;
                    }
                }
                i as u8
            }

            let mut bytes = vec![];
            for i in 0..10u32 {
                bytes.push(mod_reduce(i));
            }
            //println!("Will send bytes: {}-{:?}", bytes.len(), &bytes[0..20]);
            // send.send(SendMessage::Broadcast(Broadcast::Gossip, bytes))
            //     .await
            //     .expect("channel failure");

            //START BUILD A STABLE CONNECTED.
            // relay peer_id f1e5d319168ed41ba15e3b94ba278792bcd0c244c590cf1123a629b368a9bc9f
            // direct peer_id 3e5730bf4a3a1234d97c6c6e5b90b15b7df480e3a269dacf0d50f828162f575e
            let peer_id = PeerId::from_hex(
                "3e5730bf4a3a1234d97c6c6e5b90b15b7df480e3a269dacf0d50f828162f575e",
            )
            .unwrap(); // test peer_id
            send.send(SendMessage::StableConnect(peer_id, None, bytes))
                .await
                .expect("channel failure");
        }

        let mut first_data = true;

        while let Ok(message) = recv.recv().await {
            match message {
                ReceiveMessage::Data(peer_id, bytes) => {
                    println!(
                        "Recv permissionless data from: {}, {}-{:?}, start build a stable connection",
                        peer_id.short_show(),
                        bytes.len(),
                        bytes
                    );

                    if first_data {
                        let _ = send.send(SendMessage::Data(peer_id, bytes)).await;
                        first_data = false;
                    }
                }
                ReceiveMessage::Stream(..) => {
                    panic!("Nerver here (stream)");
                }
                ReceiveMessage::StableConnect(from, data) => {
                    println!("Recv peer what to build a stable connected: {:?}", data);

                    send.send(SendMessage::StableResult(
                        from,
                        true,
                        false,
                        vec![3, 3, 3, 3],
                    ))
                    .await
                    .expect("channel failure");
                }
                ReceiveMessage::StableLeave(peer_id) => {
                    println!("Recv stable connected leave: {}", peer_id.to_hex());
                }
                ReceiveMessage::StableResult(peer_id, is_ok, remark) => {
                    println!(
                        "Recv stable connected result: {} {} {:?}",
                        peer_id.to_hex(),
                        is_ok,
                        remark
                    );

                    let _ = send
                        .send(SendMessage::Data(peer_id, vec![1, 2, 3, 4, 5]))
                        .await;

                    // TODO START BUILD A STREAM TO TEST.
                }
            }
        }
    });
}
