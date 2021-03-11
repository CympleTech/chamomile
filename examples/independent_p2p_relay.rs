use chamomile::prelude::{start, Config, ReceiveMessage, SendMessage};
use simplelog::{CombinedLogger, Config as LogConfig, LevelFilter, WriteLogger};
use std::env::args;
use std::net::SocketAddr;
use std::path::PathBuf;

fn main() {
    CombinedLogger::init(vec![WriteLogger::new(
        LevelFilter::Debug,
        LogConfig::default(),
        std::fs::File::create(PathBuf::from("./log.txt")).unwrap(),
    )])
    .unwrap();

    smol::block_on(async {
        let addr_str = args().nth(1).expect("missing path");
        let self_addr: SocketAddr = addr_str.parse().expect("invalid addr");

        println!("START A INDEPENDENT P2P RELAY SERVER. : {}", self_addr);

        let mut config = Config::default(self_addr);
        config.permission = false;
        config.only_stable_data = true;
        config.db_dir = std::path::PathBuf::from("./");

        let (peer_id, send, recv) = start(config).await.unwrap();
        println!("peer id: {}", peer_id.to_hex());

        while let Ok(message) = recv.recv().await {
            match message {
                ReceiveMessage::Data(..) => {
                    panic!("none");
                }
                ReceiveMessage::Stream(..) => {
                    panic!("none");
                }
                ReceiveMessage::StableConnect(from, ..) => {
                    let _ = send
                        .send(SendMessage::StableResult(0, from, false, false, vec![]))
                        .await;
                }
                ReceiveMessage::ResultConnect(from, ..) => {
                    let _ = send
                        .send(SendMessage::StableResult(0, from, false, false, vec![]))
                        .await;
                }
                ReceiveMessage::StableLeave(..) => {
                    panic!("none");
                }
                ReceiveMessage::StableResult(..) => {
                    panic!("none");
                }
                ReceiveMessage::Delivery(..) => {
                    panic!("none");
                }
                ReceiveMessage::NetworkLost => {}
            }
        }
    });
}
