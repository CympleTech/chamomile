use simplelog::{CombinedLogger, Config as LogConfig, LevelFilter, TermLogger, TerminalMode};
use std::env::args;
use std::net::SocketAddr;

use chamomile::prelude::{start, Config, ReceiveMessage, SendMessage};

fn main() {
    CombinedLogger::init(vec![TermLogger::new(
        LevelFilter::Debug,
        LogConfig::default(),
        TerminalMode::Mixed,
    )
    .unwrap()])
    .unwrap();

    smol::block_on(async {
        let self_addr: SocketAddr = args()
            .nth(1)
            .expect("missing path")
            .parse()
            .expect("invalid addr");

        let mut config = Config::default(self_addr);
        config.permission = true;

        let (peer_id, send, recv) = start(config).await.unwrap();
        println!("peer id: {}", peer_id.to_hex());

        if args().nth(2).is_some() {
            let remote_addr: SocketAddr = args().nth(2).unwrap().parse().expect("invalid addr");
            println!("start connect to remote: {}", remote_addr);
            send.send(SendMessage::Connect(remote_addr, Some(vec![1])))
                .await
                .expect("channel failure!");
        }

        while let Ok(message) = recv.recv().await {
            match message {
                ReceiveMessage::Data(peer_id, bytes) => {
                    println!("Recv data from: {}, {:?}", peer_id.short_show(), bytes);
                }
                ReceiveMessage::PeerJoin(peer_id, addr, join_data) => {
                    println!(
                        "Peer join: {:?} {}, join data: {:?}",
                        peer_id, addr, join_data
                    );
                    send.send(SendMessage::PeerJoinResult(peer_id, true, false, vec![1]))
                        .await
                        .expect("channel failure!");
                }
                ReceiveMessage::PeerJoinResult(peer_id, is_ok, data) => {
                    println!(
                        "Peer Join Result: {:?} {}, data: {:?}",
                        peer_id, is_ok, data
                    );
                }
                ReceiveMessage::PeerLeave(peer_id) => {
                    println!("Peer_leave: {:?}", peer_id);
                }
                ReceiveMessage::Stream(..) => {
                    panic!("Not stream");
                }
            }
        }
    });
}
