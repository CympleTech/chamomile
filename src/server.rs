use async_std::io::Result;
use async_std::sync::{Sender, Receiver};

use crate::{PeerId, Message, Config};

struct PublicKey;
struct PrivateKey;
struct PeerList;
struct Transport;

pub struct Server {
    peer_id: PublicKey,
    peer_psk: PrivateKey,
    peer_list: PeerList,
    while_list: PeerList,
    black_list: PeerList,
    default_transport: Transport,
    support_transports: Vec<Transport>,
}

impl Server {
    pub fn new(out_send: Sender<Message>, self_recv: Receiver<Message>, config: Config) -> Self {
        Self {
            peer_id: PublicKey,
            peer_psk: PrivateKey,
            peer_list: PeerList,
            while_list: PeerList,
            black_list: PeerList,
            default_transport: Transport,
            support_transports: vec![],
        }
    }

    pub async fn start(&self) -> Result<()> {
        Ok(())
    }
}
