use chamomile_types::{
    message::ReceiveMessage,
    types::{PeerId, TransportType},
};
use postcard::{from_bytes, to_allocvec};
use serde::{Deserialize, Serialize};
use smol::future::FutureExt as SmolFutureExt;
use smol::{
    channel::{self, Receiver, RecvError, Sender},
    io::{BufReader, Result},
    lock::{Mutex, RwLock},
    prelude::*,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::hole_punching::{nat, Hole, DHT};
use crate::keys::{KeyType, Keypair, SessionKey};
use crate::peer::Peer;
use crate::peer_list::PeerList;
use crate::transports::{EndpointSendMessage, EndpointStreamMessage};

/// server send to session message in channel.
pub(crate) enum SessionSendMessage {
    /// send bytes to session what want to send to peer..
    Bytes(PeerId, PeerId, Vec<u8>),
    /// when need build a stable connection.
    StableConnect(PeerId, Vec<u8>),
    /// when receive a stable result.
    StableResult(PeerId, bool, Vec<u8>),
    /// close the session.
    Close,
}

/// new a channel for send message to session.
pub(crate) fn new_session_send_channel(
) -> (Sender<SessionSendMessage>, Receiver<SessionSendMessage>) {
    channel::unbounded()
}

/// start a session to handle incoming.
pub(crate) async fn start(
    remote_addr: SocketAddr,
    endpoint_recv: Receiver<EndpointStreamMessage>,
    endpoint_send: Sender<EndpointStreamMessage>,
    out_sender: Sender<ReceiveMessage>,
    key: Arc<Keypair>,
    peer: Arc<Peer>,
    peer_list: Arc<RwLock<PeerList>>,
    transports: Arc<RwLock<HashMap<TransportType, Sender<EndpointSendMessage>>>>,
    is_permissioned: bool,
    is_stable: Option<Vec<u8>>,
) {
    let my_peer_id = peer.id().clone();

    // timeout 10s to read peer_id & public_key
    let result: Result<Option<RemotePublic>> = endpoint_recv
        .recv()
        .or(async {
            smol::Timer::after(Duration::from_secs(10)).await;
            Err(RecvError)
        })
        .await
        .map(|msg| match msg {
            EndpointStreamMessage::Bytes(bytes) => RemotePublic::from_bytes(key.key, bytes).ok(),
            _ => None,
        })
        .map_err(|_e| std::io::ErrorKind::TimedOut.into());

    if result.is_err() {
        debug!("Debug: Session timeout");
        let _ = endpoint_send.send(EndpointStreamMessage::Close).await;
        return;
    }
    let result = result.unwrap();
    if result.is_none() {
        debug!("Debug: Session invalid pk");
        let _ = endpoint_send.send(EndpointStreamMessage::Close).await;
        return;
    }

    let RemotePublic(remote_peer_key, remote_peer) = result.unwrap();
    let remote_peer_id = remote_peer_key.peer_id();
    let session_key: SessionKey = key.key.session_key(&key, &remote_peer_key);

    debug!("Debug: Session connected: {}", remote_peer_id.short_show());
    let (sender, receiver) = new_session_send_channel();
    let remote_transport = nat(remote_addr, remote_peer);
    debug!("Debug: NAT addr: {}", remote_transport.addr());

    // check and save tmp and save outside
    if remote_peer_id == my_peer_id || peer_list.read().await.is_black_peer(&remote_peer_id) {
        return;
    }

    // save to peer_list.
    let mut peer_list_lock = peer_list.write().await;
    let is_new = peer_list_lock
        .peer_add(remote_peer_id, sender.clone(), remote_peer)
        .await;
    drop(peer_list_lock);

    if !is_new {
        debug!("Session is had connected");
        return;
    }

    endpoint_send
        .send(EndpointStreamMessage::Bytes(
            RemotePublic(key.public(), *peer.clone()).to_bytes(),
        ))
        .await
        .expect("Session to Endpoint (Data Key)");

    endpoint_send
        .send(EndpointStreamMessage::Bytes(
            SessionType::Key(session_key.out_bytes()).to_bytes(),
        ))
        .await
        .expect("Session to Endpoint (Data SessionKey)");

    let sign = vec![]; // TODO
    let peers = peer_list.read().await.get_dht_help(&remote_peer_id);
    endpoint_send
        .send(EndpointStreamMessage::Bytes(
            SessionType::DHT(DHT(peers), sign).to_bytes(),
        ))
        .await
        .expect("Sesssion to Endpoint (Data)");

    let is_stabled = is_stable.is_some();
    if let Some(data) = is_stable {
        let data = SessionType::StableConnect(remote_peer_id, data).to_bytes();
        endpoint_send
            .send(EndpointStreamMessage::Bytes(data))
            .await
            .expect("Session to Endpoint (Stable)");
    }

    let is_stabled = Arc::new(Mutex::new(is_stabled));

    let session = Session {
        my_peer_id,
        remote_peer_id,
        remote_peer,
        endpoint_recv,
        endpoint_send,
        out_sender,
        receiver,
        sender,
        session_key,
        key,
        peer,
        peer_list,
        transports,
        is_permissioned,
        is_stabled,
    };

    session.listen().await
}

struct Session {
    my_peer_id: PeerId,
    remote_peer_id: PeerId,
    remote_peer: Peer,
    endpoint_recv: Receiver<EndpointStreamMessage>,
    endpoint_send: Sender<EndpointStreamMessage>,
    out_sender: Sender<ReceiveMessage>,
    receiver: Receiver<SessionSendMessage>,
    sender: Sender<SessionSendMessage>,
    session_key: SessionKey,
    key: Arc<Keypair>,
    peer: Arc<Peer>,
    peer_list: Arc<RwLock<PeerList>>,
    transports: Arc<RwLock<HashMap<TransportType, Sender<EndpointSendMessage>>>>,
    is_permissioned: bool,
    is_stabled: Arc<Mutex<bool>>,
}

impl Session {
    async fn listen(&self) {
        loop {
            if self
                .listen_outside()
                .race(self.listen_endpoint())
                .await
                .is_err()
            {
                break;
            }
        }

        debug!("Session broke: {:?}", self.remote_peer_id);
    }

    async fn listen_outside(&self) -> Result<()> {
        match self.receiver.recv().await {
            Ok(SessionSendMessage::Bytes(from, to, bytes)) => {
                let e_data = self.session_key.encrypt(bytes);
                let data = SessionType::Data(from, to, e_data).to_bytes();
                self.endpoint_send
                    .send(EndpointStreamMessage::Bytes(data))
                    .await
                    .expect("Session to Endpoint (Data)");
            }
            Ok(SessionSendMessage::Close) => {
                self.endpoint_send
                    .send(EndpointStreamMessage::Close)
                    .await
                    .expect("Session to Endpoint (Close)");
                if *self.is_stabled.lock().await {
                    self.out_sender
                        .send(ReceiveMessage::StableLeave(self.remote_peer_id))
                        .await
                        .expect("Session to Outside (Close)");
                }
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "session closed",
                ));
            }
            Ok(SessionSendMessage::StableConnect(to, data)) => {
                debug!("Session recv stable connect to: {:?}", to);
                let mut guard = self.is_stabled.lock().await;
                *guard = true;
                drop(guard);

                // Start stable connect.
                let data = SessionType::StableConnect(to, data).to_bytes();
                self.endpoint_send
                    .send(EndpointStreamMessage::Bytes(data))
                    .await
                    .expect("Session to Endpoint (Stable)");
            }
            Ok(SessionSendMessage::StableResult(to, is_ok, data)) => {
                let data = SessionType::StableResult(to, is_ok, data).to_bytes();
                self.endpoint_send
                    .send(EndpointStreamMessage::Bytes(data))
                    .await
                    .expect("Session to Endpoint (Stable)");

                // TODO build stable connections.
                if is_ok {
                    let mut guard = self.is_stabled.lock().await;
                    *guard = true;
                    drop(guard);

                    self.peer_list.write().await.stable_add(
                        self.remote_peer_id,
                        self.sender.clone(),
                        self.remote_peer.clone(),
                    );
                    // TODO build stable connections.
                    debug!("TODO more stable connections: receiver");
                }
            }
            Err(e) => {}
        }

        Ok(())
    }

    async fn listen_endpoint(&self) -> Result<()> {
        match self.endpoint_recv.recv().await {
            Ok(EndpointStreamMessage::Bytes(bytes)) => {
                let t = SessionType::from_bytes(bytes);
                if t.is_err() {
                    debug!("Debug: Error Serialize SessionType");
                }

                match t.unwrap() {
                    SessionType::Key(bytes) => {
                        // if !session_key.is_ok() {
                        //     if !session_key.in_bytes(bytes) {
                        //         server_send
                        //             .send(SessionReceiveMessage::Close(remote_peer_id))
                        //             .await
                        //             .expect("Session to Server (Key Close)");
                        //         endpoint_send
                        //             .send(EndpointStreamMessage::Close)
                        //             .await
                        //             .expect("Session to Endpoint (Key Close)");
                        //         break;
                        //     }

                        //     endpoint_send
                        //         .send(EndpointStreamMessage::Bytes(
                        //             SessionType::Key(session_key.out_bytes()).to_bytes(),
                        //         ))
                        //         .await
                        //         .expect("Session to Endpoint (Bytes)");
                        // }

                        // while !buffers.is_empty() {
                        //     let (peer_id, bytes) = buffers.pop().unwrap();
                        //     let e_data = session_key.encrypt(bytes);
                        //     let data =
                        //         SessionType::Data(my_peer_id, peer_id, e_data).to_bytes();
                        //     endpoint_send
                        //         .send(EndpointStreamMessage::Bytes(data))
                        //         .await
                        //         .expect("Session to Endpoint (Bytes)");
                        // }

                        // while !receiver_buffers.is_empty() {
                        //     let e_data = receiver_buffers.pop().unwrap();
                        //     let d_data = session_key.decrypt(e_data);
                        //     if d_data.is_ok() {
                        //         out_sender
                        //             .send(ReceiveMessage::Data(remote_peer_id, d_data.unwrap()))
                        //             .await
                        //             .expect("Session to Outside (Data)");
                        //     }
                        // }
                    }
                    SessionType::Data(from, to, e_data) => {
                        // if !session_key.is_ok() {
                        //     continue;
                        // }

                        let d_data = self.session_key.decrypt(e_data);
                        if d_data.is_ok() {
                            if to == self.my_peer_id {
                                if self.is_permissioned {
                                    //
                                } else {
                                    self.out_sender
                                        .send(ReceiveMessage::Data(from, d_data.unwrap()))
                                        .await
                                        .expect("Session to Outside (Data)");
                                }
                            } else {
                                // Relay
                                let peer_list_lock = self.peer_list.read().await;
                                let sender = peer_list_lock.get(&to);
                                if sender.is_some() {
                                    let sender = sender.unwrap();
                                    sender
                                        .send(SessionSendMessage::Bytes(from, to, d_data.unwrap()))
                                        .await
                                        .expect("Session to Session (Relay)");
                                }
                            }
                        }
                    }
                    SessionType::DHT(peers, _sign) => {
                        let DHT(peers) = peers;
                        // TODO DHT Helper
                        // remote_peer_key.verify()

                        if peers.len() > 0 {
                            let my_rp =
                                RemotePublic(self.key.public(), *self.peer.clone()).to_bytes();

                            for p in peers {
                                if p.id() != &self.my_peer_id
                                    && self.peer_list.read().await.get_it(p.id()).is_none()
                                {
                                    if let Some(sender) =
                                        self.transports.read().await.get(p.transport())
                                    {
                                        sender
                                            .send(EndpointSendMessage::Connect(
                                                *p.addr(),
                                                my_rp.clone(),
                                                None,
                                            ))
                                            .await
                                            .expect("Server to Endpoint (Connect)");
                                    }
                                }
                            }
                        }
                    }
                    SessionType::Ping => {
                        self.endpoint_send
                            .send(EndpointStreamMessage::Bytes(SessionType::Pong.to_bytes()))
                            .await
                            .expect("Session to Endpoint (Ping)");
                    }
                    SessionType::Pong => {
                        todo!();
                        // TODO Heartbeat Ping/Pong
                    }
                    SessionType::Hole(..) => {
                        todo!();
                    }
                    SessionType::HoleConnect => {
                        todo!();
                    }
                    SessionType::StableConnect(to, data) => {
                        debug!("Recv stable connect from: {:?}", self.remote_peer_id);
                        if to == self.my_peer_id {
                            self.out_sender
                                .send(ReceiveMessage::StableConnect(self.remote_peer_id, data))
                                .await
                                .expect("Session to Outside (Data)");
                        } else {
                            // TODO proxy
                        }
                    }
                    SessionType::StableResult(to, is_ok, data) => {
                        debug!(
                            "Recv stable connect result {} from: {:?}",
                            is_ok, self.remote_peer_id
                        );
                        if to == self.my_peer_id {
                            self.out_sender
                                .send(ReceiveMessage::StableResult(
                                    self.remote_peer_id,
                                    is_ok,
                                    data,
                                ))
                                .await
                                .expect("Session to Outside (Data)");

                            if is_ok && *self.is_stabled.lock().await {
                                self.peer_list.write().await.stable_add(
                                    self.remote_peer_id,
                                    self.sender.clone(),
                                    self.remote_peer.clone(),
                                );
                                // TODO build stable connections.
                                debug!("TODO more stable connections: sender");
                            }
                        } else {
                            // TODO proxy
                        }
                    }
                }
            }
            Ok(EndpointStreamMessage::Close) => {
                self.peer_list
                    .write()
                    .await
                    .peer_remove(&self.remote_peer_id);

                if self.is_permissioned {
                    self.peer_list
                        .write()
                        .await
                        .stable_leave(&self.remote_peer_id);
                }

                if *self.is_stabled.lock().await {
                    self.out_sender
                        .send(ReceiveMessage::StableLeave(self.remote_peer_id))
                        .await
                        .expect("Session to Outside (Close)");
                }
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "session closed",
                ));
            }
            Err(_) => {
                if *self.is_stabled.lock().await {
                    self.out_sender
                        .send(ReceiveMessage::StableLeave(self.remote_peer_id))
                        .await
                        .expect("Session to Outside (Close)");
                }
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "session closed",
                ));
            }
        }

        Ok(())
    }
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
    StableConnect(PeerId, Vec<u8>),
    StableResult(PeerId, bool, Vec<u8>),
}

impl SessionType {
    fn to_bytes(&self) -> Vec<u8> {
        to_allocvec(self).unwrap_or(vec![])
    }

    fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        from_bytes(&bytes).map_err(|_e| {
            std::io::Error::new(std::io::ErrorKind::Other, "serialize session type error")
        })
    }
}

/// Rtemote Public Info, include local transport and public key bytes.
#[derive(Deserialize, Serialize, Debug)]
pub struct RemotePublic(pub Keypair, pub Peer);

impl RemotePublic {
    pub fn from_bytes(key: KeyType, bytes: Vec<u8>) -> Result<Self> {
        from_bytes(&bytes).map_err(|_e| {
            std::io::Error::new(std::io::ErrorKind::Other, "serialize remote public error")
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let bytes = to_allocvec(self).unwrap_or(vec![]);
        bytes
    }
}
