use chamomile::prelude::{start, Config, Peer, ReceiveMessage, SendMessage};
use std::{env::args, net::SocketAddr};
use tracing::info;

#[tokio::main]
async fn main() {
    std::env::set_var("RUST_LOG", "info");

    tracing_subscriber::fmt::init();

    let addr_str = args().nth(1).unwrap_or("0.0.0.0:7364".to_owned());
    let self_addr: SocketAddr = addr_str.parse().expect("invalid addr");

    info!("START A INDEPENDENT P2P RELAY SERVER: {}", self_addr);

    let mut config = Config::default(Peer::socket(self_addr));
    config.permission = false;
    config.only_stable_data = true;
    config.db_dir = std::path::PathBuf::from(".data/relay");

    let (peer_id, send, mut recv) = start(config).await.unwrap();
    info!("peer id: {}", peer_id.to_hex());

    if args().nth(2).is_some() {
        let remote_addr: SocketAddr = args().nth(2).unwrap().parse().expect("invalid addr");
        info!("start DHT connect to remote: {}", remote_addr);
        send.send(SendMessage::Connect(Peer::socket(remote_addr)))
            .await
            .expect("channel failure");
    }

    while let Some(message) = recv.recv().await {
        match message {
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
            _ => {}
        }
    }
}
