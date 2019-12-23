use async_std::{
    io::Result,
    sync::{Arc, Mutex, Receiver, Sender},
    task,
};
use std::collections::HashMap;
use std::net::SocketAddr;

use crate::transports::TcpEndpoint;
use crate::transports::UdpEndpoint;
use crate::transports::{new_channel, Endpoint, EndpointMessage};

use super::keys::{KeyType, PrivateKey, PublicKey};
use super::peer_id::PeerId;

use crate::{Config, Message};

#[derive(Debug)]
struct PeerList;

#[derive(Debug, Hash)]
enum Transport {
    UDP(SocketAddr), // 0u8
    TCP(SocketAddr), // 1u8
}

impl Transport {
    fn symbol(&self) -> u8 {
        match self {
            &Transport::UDP(_) => 0u8,
            &Transport::TCP(_) => 1u8,
        }
    }
}

pub struct Server {
    peer_id: PeerId,
    pk: PublicKey,
    psk: PrivateKey,
    peer_list: PeerList,
    while_list: PeerList,
    black_list: PeerList,
    default_transport: Transport,
    transports: HashMap<u8, Sender<EndpointMessage>>,
}

impl Server {
    fn new(config: Config) -> Self {
        // load or generate keypair
        let (psk, pk) = PrivateKey::generate(KeyType::Ed25519);
        let peer_id = pk.peer_id();

        Self {
            peer_id,
            pk,
            psk,
            peer_list: PeerList,
            while_list: PeerList,
            black_list: PeerList,
            default_transport: Transport::UDP(config.addr),
            transports: HashMap::new(),
        }
    }

    pub async fn start(
        config: Config,
        out_send: Sender<Message>,
        self_recv: Receiver<Message>,
    ) -> Result<()> {
        let server = Self::new(config);
        println!("server start peer id: {:?}", server.peer_id);

        // mock
        let (send, recv) = new_channel();
        let transport_send = match server.default_transport {
            Transport::UDP(addr) => UdpEndpoint::start(addr, server.peer_id.clone(), send).await?,
            Transport::TCP(addr) => TcpEndpoint::start(addr, server.peer_id.clone(), send).await?,
        };

        let m1 = Arc::new(Mutex::new(server));
        let m2 = m1.clone();

        task::spawn(async move {
            while let Some(message) = recv.recv().await {
                println!("Server: recv from transport: {:?}", message);
                let server = m1.lock().await;
                println!("Server: server: {:?}", server.peer_id);
                out_send
                    .send(Message::Data(vec![1, 2, 3, 4], PeerId::default()))
                    .await;
            }
        });

        task::spawn(async move {
            while let Some(message) = self_recv.recv().await {
                println!("Server: recv from transport: {:?}", message);
                let server = m2.lock().await;
                println!("Server: server: {:?}", server.peer_id);
                match message {
                    Message::Connect(addr) => {
                        transport_send.send(EndpointMessage::Connect(addr)).await
                    }
                    Message::DisConnect(addr) => {
                        transport_send.send(EndpointMessage::Disconnect(addr)).await
                    }
                    _ => {}
                }
            }
        });

        Ok(())
    }
}
