use futures::{select, FutureExt};
use postcard::{from_bytes, to_allocvec};
use serde::{Deserialize, Serialize};
use smol::future::FutureExt as SmolFutureExt;
use smol::{
    channel::{self, Receiver, RecvError, Sender},
    io,
    io::BufReader,
    lock::RwLock,
    prelude::*,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use chamomile_types::{message::ReceiveMessage, types::PeerId};

use crate::hole_punching::{nat, Hole, DHT};
use crate::keys::{KeyType, Keypair, SessionKey};
use crate::peer::Peer;
use crate::peer_list::PeerList;
use crate::transports::{EndpointSendMessage, EndpointStreamMessage};

/// server send to session message in channel.
pub(crate) enum SessionSendMessage {
    /// when peer join ok, will send info to session, bool is stabled.
    Ok(Vec<u8>, bool),
    /// when peer join failure or close by self.
    Close,
    /// send bytes to session what want to send to peer..
    Bytes(PeerId, PeerId, Vec<u8>),
}

/// server receive from session message in channel.
pub(crate) enum SessionReceiveMessage {
    /// when connection is ok, will send info to server.
    Connected(PeerId, Sender<SessionSendMessage>, Peer, Vec<u8>, bool),
    /// when connection is closed.
    Close(PeerId),
    /// start try to connect this peer. from DHT help.
    Connect(SocketAddr),
}

/// new a channel for receive message from session.
pub(crate) fn new_session_receive_channel() -> (
    Sender<SessionReceiveMessage>,
    Receiver<SessionReceiveMessage>,
) {
    channel::unbounded()
}

/// new a channel for send message to session.
pub(crate) fn new_session_send_channel(
) -> (Sender<SessionSendMessage>, Receiver<SessionSendMessage>) {
    channel::unbounded()
}

/// start a session to handle incoming.
pub(crate) fn start(
    remote_addr: SocketAddr,
    endpoint_recv: Receiver<EndpointStreamMessage>,
    endpoint_send: Sender<EndpointStreamMessage>,
    server_send: Sender<SessionReceiveMessage>,
    out_sender: Sender<ReceiveMessage>,
    key: Arc<Keypair>,
    peer: Arc<Peer>,
    peer_list: Arc<RwLock<PeerList>>,
    mut is_stable: bool,
    is_permissioned: bool,
) {
    smol::spawn(async move {
        // timeout 10s to read peer_id & public_key
        let result: io::Result<Option<RemotePublic>> = endpoint_recv
            .recv()
            .or(async {
                smol::Timer::after(Duration::from_secs(10)).await;
                Err(RecvError)
            })
            .await
            .map(|msg| match msg {
                EndpointStreamMessage::Bytes(bytes) => {
                    RemotePublic::from_bytes(key.key, bytes).ok()
                }
                _ => None,
            })
            .map_err(|_e| std::io::ErrorKind::TimedOut.into());

        if result.is_err() {
            debug!("Debug: Session timeout");
            let _ = endpoint_send
                .send(EndpointStreamMessage::Close)
                .await;
            drop(endpoint_recv);
            drop(endpoint_send);
            return;
        }
        let result = result.unwrap();
        if result.is_none() {
            debug!("Debug: Session invalid pk");
            let _ = endpoint_send
                .send(EndpointStreamMessage::Close)
                .await;
            drop(endpoint_recv);
            drop(endpoint_send);
            return;
        }

        let RemotePublic(remote_peer_key, remote_local_addr, remote_join_data) = result.unwrap();
        let remote_peer_id = remote_peer_key.peer_id();
        let mut session_key: SessionKey = key.key.session_key(&key, &remote_peer_key);

        debug!("Debug: Session connected: {}", remote_peer_id.short_show());
        let (sender, receiver) = new_session_send_channel();
        let remote_transport = nat(remote_addr, remote_local_addr);
        debug!("Debug: NAT addr: {}", remote_transport.addr());
        server_send
            .send(SessionReceiveMessage::Connected(
                remote_peer_id,
                sender,
                remote_transport,
                remote_join_data,
                is_stable,
            ))
            .await
            .expect("Session to Server (Connected)");

        let mut buffers: Vec<(PeerId, Vec<u8>)> = vec![];
        let mut receiver_buffers: Vec<Vec<u8>> = vec![];
        let my_peer_id = peer.id().clone();

        loop {
            select! {
                msg = endpoint_recv.recv().fuse() => match msg {
                    Ok(EndpointStreamMessage::Bytes(bytes)) => {
                        let t = SessionType::from_bytes(bytes);
                        if t.is_err() {
                            debug!("Debug: Error Serialize SessionType");
                            continue;
                        }

                        match t.unwrap() {
                            SessionType::Key(bytes) => {
                                if !session_key.is_ok() {
                                    if !session_key.in_bytes(bytes) {
                                        server_send.send(SessionReceiveMessage::Close(
                                            remote_peer_id)
                                        ).await.expect("Session to Server (Key Close)");
                                        endpoint_send.send(
                                            EndpointStreamMessage::Close
                                        ).await.expect("Session to Endpoint (Key Close)");
                                        break;
                                    }

                                    endpoint_send.send(EndpointStreamMessage::Bytes(
                                        SessionType::Key(session_key.out_bytes())
                                            .to_bytes()
                                    )).await.expect("Session to Endpoint (Bytes)");
                                }

                                while !buffers.is_empty() {
                                    let (peer_id, bytes) = buffers.pop().unwrap();
                                    let e_data = session_key.encrypt(bytes);
                                    let data = SessionType::Data(
                                        my_peer_id,
                                        peer_id,
                                        e_data
                                    ).to_bytes();
                                    endpoint_send.send(
                                        EndpointStreamMessage::Bytes(data)
                                    ).await.expect("Session to Endpoint (Bytes)");
                                }

                                while !receiver_buffers.is_empty() {
                                    let e_data = receiver_buffers.pop().unwrap();
                                    let d_data = session_key.decrypt(e_data);
                                    if d_data.is_ok() {
                                        out_sender
                                            .send(ReceiveMessage::Data(
                                                remote_peer_id,
                                                d_data.unwrap()
                                            )).await.expect("Session to Outside (Data)");
                                    }
                                }

                            }
                            SessionType::Data(from, to, e_data) => {
                                if !session_key.is_ok() {
                                    continue;
                                }

                                let d_data = session_key.decrypt(e_data);
                                if d_data.is_ok() {
                                    if to == my_peer_id {
                                        if is_permissioned && !is_stable {
                                            continue;
                                        }

                                        out_sender.send(ReceiveMessage::Data(
                                            from,
                                            d_data.unwrap()
                                        )).await.expect("Session to Outside (Data)");
                                    } else {
                                        // Relay
                                        let peer_list_lock = peer_list.read().await;
                                        let sender = peer_list_lock.get(&to);
                                        if sender.is_some() {
                                            let sender = sender.unwrap();
                                            sender.send(SessionSendMessage::Bytes(
                                                from,
                                                to,
                                                d_data.unwrap()
                                            )).await.expect("Session to Session (Relay)");
                                        }
                                    }
                                }
                            }
                            SessionType::DHT(peers, _sign) => {
                                let DHT(peers) = peers;
                                // TODO DHT Helper
                                // remote_peer_key.verify()

                                for p in peers {
                                    if p.id() != peer.id() &&
                                        peer_list.read().await.get_it(p.id()).is_none()
                                    {
                                        server_send.send(SessionReceiveMessage::Connect(
                                            *p.addr(),
                                        )).await.expect("Session to Server (Connect)");
                                    }
                                }

                            }
                            SessionType::Ping => {
                                endpoint_send.send(EndpointStreamMessage::Bytes(
                                    SessionType::Pong.to_bytes()
                                )).await.expect("Session to Endpoint (Ping)");
                            }
                            SessionType::Pong => {
                                todo!();
                                // TODO Heartbeat Ping/Pong
                            },
                            SessionType::Hole(..) => {
                                todo!();
                            },
                            SessionType::HoleConnect => {
                                todo!();
                            }
                        }
                    },
                    Ok(EndpointStreamMessage::Close) => {
                        server_send.send(SessionReceiveMessage::Close(remote_peer_id))
                            .await.expect("Session to Server (Close by Endpoint)");
                        break;
                    }
                    Err(_) => {
                        break;
                    }
                },
                out_msg = receiver.recv().fuse() => match out_msg {
                    Ok(SessionSendMessage::Bytes(from, to, bytes)) => {
                        if session_key.is_ok() {
                            let e_data = session_key.encrypt(bytes);
                            let data = SessionType::Data(from, to, e_data).to_bytes();
                            endpoint_send.send(EndpointStreamMessage::Bytes(data))
                                .await.expect("Session to Endpoint (Data)");
                        } else {
                            buffers.push((to, bytes));
                        }
                    },
                    Ok(SessionSendMessage::Ok(data, stable)) => {
                        is_stable = stable;

                        if is_stable {
                            // todo!();
                            debug!("TODO Stable connection");
                        }

                        endpoint_send.send(EndpointStreamMessage::Bytes(
                            RemotePublic(
                                key.public(),
                                *peer.clone(),
                                data
                            ).to_bytes()
                        )).await.expect("Session to Endpoint (Data Key)");

                        endpoint_send.send(EndpointStreamMessage::Bytes(
                            SessionType::Key(session_key.out_bytes()).to_bytes()
                        )).await.expect("Session to Endpoint (Data SessionKey)");

                        let sign = vec![]; // TODO
                        let peers = peer_list.read().await.get_dht_help(&remote_peer_id);
                        endpoint_send.send(EndpointStreamMessage::Bytes(
                            SessionType::DHT(DHT(peers), sign).to_bytes()
                        )).await.expect("Sesssion to Endpoint (Data)");
                    },
                    Ok(SessionSendMessage::Close) => {
                        endpoint_send.send(EndpointStreamMessage::Close).await.expect("Session to Endpoint (Close)");
                        break;
                    }
                    Err(e) => {
                        break;
                    }
                }
            }
        }
    }).detach();
}

#[derive(Deserialize, Serialize)]
enum SessionType {
    Key(Vec<u8>),
    Data(PeerId, PeerId, Vec<u8>),
    DHT(DHT, Vec<u8>),
    Hole(Hole),
    HoleConnect,
    Ping,
    Pong,
}

impl SessionType {
    fn to_bytes(&self) -> Vec<u8> {
        to_allocvec(self).unwrap_or(vec![])
    }

    fn from_bytes(bytes: Vec<u8>) -> Result<Self, ()> {
        from_bytes(&bytes).map_err(|_e| ())
    }
}

/// Rtemote Public Info, include local transport and public key bytes.
#[derive(Deserialize, Serialize, Debug)]
pub struct RemotePublic(pub Keypair, pub Peer, pub Vec<u8>);

impl RemotePublic {
    pub fn from_bytes(key: KeyType, bytes: Vec<u8>) -> Result<Self, ()> {
        from_bytes(&bytes).map_err(|_e| ())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let bytes = to_allocvec(self).unwrap_or(vec![]);
        bytes
    }
}
