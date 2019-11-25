use async_std::io::Result;
use async_std::sync::{Sender, Receiver};
use async_std::sync::{Arc, Mutex};
use async_std::task;
use std::net::SocketAddr;

use crate::{PeerId, Message, Config};

#[derive(Debug)]
struct PublicKey;

#[derive(Debug)]
struct PrivateKey;

#[derive(Debug)]
struct PeerList;

#[derive(Debug)]
enum Transport {
    UDP(SocketAddr)
}

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
    pub fn new(config: Config) -> Self {
        Self {
            peer_id: PublicKey,
            peer_psk: PrivateKey,
            peer_list: PeerList,
            while_list: PeerList,
            black_list: PeerList,
            default_transport: Transport::UDP(config.addr),
            support_transports: vec![],
        }
    }

    pub async fn start(server: Server, out_send: Sender<Message>, self_recv: Receiver<Message>) -> Result<()> {
        // mock
        use crate::transports::UdpEndpoint;
        use crate::transports::Endpoint;
        use crate::transports::new_channel;

        let (send, recv) = new_channel();
        let transport_send = match server.default_transport {
            Transport::UDP(addr) => UdpEndpoint::start(addr, send).await?,
        };

        let m1 = Arc::new(Mutex::new(server));
        let m2 = m1.clone();

        task::spawn(async move {
            while let Some(message) = recv.recv().await {
                println!("Server: recv from transport: {:?}", message);
                let server = m1.lock().await;
                println!("Server: server: {:?}", server.peer_id);
                out_send.send(Message::Data(vec![1, 2, 3, 4], PeerId)).await;
            }
        });

        task::spawn(async move {
            while let Some(message) = self_recv.recv().await {
                println!("Server: recv from transport: {:?}", message);
                let server = m2.lock().await;
                println!("Server: server: {:?}", server.peer_id);
                match message {
                    Message::Connect(addr) => transport_send.send((vec![1], addr)).await,
                    Message::DisConnect(addr) => transport_send.send((vec![2], addr)).await,
                    _ => {}
                }
            }
        });

        Ok(())
    }
}
