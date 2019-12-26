use async_std::{
    io::Result,
    prelude::*,
    sync::{Arc, Mutex, Receiver, Sender},
    task,
};
use futures::{select, FutureExt};
use std::collections::HashMap;
use std::net::SocketAddr;

use crate::transports::TcpEndpoint;
use crate::transports::UdpEndpoint;
use crate::transports::{new_channel, Endpoint, EndpointMessage, StreamMessage};
use crate::{Config, Message};

use super::keys::KeyType;
use super::peer_list::{PeerList, PeerTable};
use super::session::{start as session_start, RemotePublic};
use super::transport::Transport;

/// start server
pub async fn start(
    config: Config,
    out_send: Sender<Message>,
    mut self_recv: Receiver<Message>,
) -> Result<()> {
    // TODO load or init config.
    // load or generate keypair
    let key = KeyType::Ed25519.generate_kepair();
    let peer_id = key.peer_id();
    let mut peer_list = PeerTable::init(peer_id);
    let _while_list = PeerList::default();
    let black_list = PeerList::default();
    let default_transport = Transport::TCP(config.addr, true);
    let _transports: HashMap<u8, Sender<EndpointMessage>> = HashMap::new();

    let (send, mut recv) = new_channel();
    let transport_send = match default_transport {
        Transport::UDP(addr, _) => UdpEndpoint::start(addr, peer_id.clone(), send.clone())
            .await
            .expect("UDP Transport binding failure!"),
        Transport::TCP(addr, _) => TcpEndpoint::start(addr, peer_id.clone(), send.clone())
            .await
            .expect("TCP Transport binding failure!"),
        _ => panic!("Not suppert, waiting"),
    };

    println!("Debug: listening: {}", config.addr);
    println!("Debug: peer id: {}", peer_id.short_show());

    task::spawn(async move {
        //let m1 = Arc::new(Mutex::new(server));
        //let m2 = m1.clone();

        loop {
            select! {
                msg = recv.next().fuse() => match msg {
                    Some(message) => {
                        match message {
                            EndpointMessage::PreConnected(addr, receiver, sender, is_ok) => {
                                // check and start session
                                if black_list.contains_addr(&addr) {
                                    sender.send(StreamMessage::Close).await;
                                } else {
                                    session_start(
                                        addr,
                                        receiver,
                                        sender,
                                        send.clone(),
                                        out_send.clone(),
                                        Arc::new(key.clone()),
                                        default_transport.clone(),
                                        is_ok,
                                    )
                                }
                            }
                            EndpointMessage::Connected(peer_id, sender, transport) => {
                                // check and save tmp and save outside
                                if black_list.contains_peer(&peer_id) {
                                    sender.send(StreamMessage::Close).await;
                                } else {
                                    peer_list.add_tmp_peer(peer_id, sender, transport);
                                    out_send.send(Message::PeerJoin(peer_id)).await;
                                }
                            }
                            EndpointMessage::Close(peer_id) => {
                                peer_list.remove(&peer_id);
                                out_send.send(Message::PeerLeave(peer_id)).await;
                            }
                            _ => {}
                        }
                    },
                    None => break,
                },
                msg = self_recv.next().fuse() => match msg {
                    Some(message) => {
                        match message {
                            Message::Connect(addr) => {
                                transport_send
                                    .send(EndpointMessage::Connect(
                                        addr,
                                        RemotePublic(key.public().clone(), default_transport.clone()).to_bytes()
                                    ))
                                    .await;
                            }
                            Message::DisConnect(addr) => {
                                transport_send.send(EndpointMessage::Disconnect(addr)).await;
                            }
                            Message::PeerJoinResult(peer_id, is_ok) => {
                                let sender = peer_list.get(&peer_id);
                                if sender.is_some() {
                                    let sender = sender.unwrap();
                                    if is_ok {
                                        sender.send(StreamMessage::Ok).await;
                                        peer_list.stabilize_tmp_peer(peer_id);
                                    } else {
                                        sender.send(StreamMessage::Close).await;
                                        peer_list.remove_tmp_peer(&peer_id);
                                    }
                                }
                            }
                            Message::Data(peer_id, data) => {
                                let sender = peer_list.get(&peer_id);
                                if sender.is_some() {
                                    let sender = sender.unwrap();
                                    sender.send(StreamMessage::Bytes(data)).await;
                                }
                            },
                            Message::PeerLeave(peer_id) => {
                                let sender = peer_list.get(&peer_id);
                                if sender.is_some() {
                                    let sender = sender.unwrap();
                                    sender.send(StreamMessage::Close).await;
                                    peer_list.remove_tmp_peer(&peer_id);
                                }
                            },
                            Message::PeerJoin(_peer_id) => {},  // TODO search peer and join
                        }
                    },
                    None => break,
                }
            }
        }
    });

    Ok(())
}
