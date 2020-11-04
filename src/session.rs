use chamomile_types::{
    message::ReceiveMessage,
    types::{PeerId, TransportType, PEER_ID_LENGTH},
};
use smol::{
    channel::{self, Receiver, RecvError, Sender},
    future,
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
use crate::peer::{Peer, PEER_LENGTH};
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
    is_only_stable_data: bool,
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
            EndpointStreamMessage::Bytes(bytes) => RemotePublic::from_bytes(bytes).ok(),
            _ => None,
        })
        .map_err(|_e| std::io::ErrorKind::TimedOut.into());

    if result.is_err() {
        debug!("Debug: Session timeout, close it.");
        let _ = endpoint_send.send(EndpointStreamMessage::Close).await;
        return;
    }
    let result = result.unwrap();
    if result.is_none() {
        debug!("Debug: Session invalid pk, close it.");
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
        debug!("session remote peer is blocked, close it.");
        let _ = endpoint_send.send(EndpointStreamMessage::Close).await;
        return;
    }

    // save to peer_list.
    let mut peer_list_lock = peer_list.write().await;
    let is_new = peer_list_lock
        .peer_add(remote_peer_id, sender.clone(), remote_peer)
        .await;
    drop(peer_list_lock);

    if !is_new {
        debug!("Session is had connected, close it.");
        let _ = endpoint_send.send(EndpointStreamMessage::Close).await;
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

    let peers = peer_list.read().await.get_dht_help(&remote_peer_id);
    endpoint_send
        .send(EndpointStreamMessage::Bytes(
            SessionType::DHT(DHT(peers)).to_bytes(),
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

    // if is_permissioned is true, only stable connected data. not relay.
    // if is_permissioned is false, is_only_stable_data is true, only stable connected data. but can relay.
    // if is_permissioned is false, is_only_stable data is false, all connnected data and can relay.
    let is_relay_data = !is_permissioned;
    let is_recv_data = !is_only_stable_data || is_stabled;

    let is_stabled = Arc::new(Mutex::new(is_stabled));

    let session = Session {
        my_peer_id,
        remote_peer_id,
        remote_peer,
        endpoint_send,
        out_sender,
        sender,
        session_key,
        key,
        peer,
        peer_list,
        transports,
        is_recv_data,
        is_relay_data,
        is_stabled,
    };

    session.listen(receiver, endpoint_recv).await
}

struct Session {
    my_peer_id: PeerId,
    remote_peer_id: PeerId,
    remote_peer: Peer,
    endpoint_send: Sender<EndpointStreamMessage>,
    out_sender: Sender<ReceiveMessage>,
    sender: Sender<SessionSendMessage>,
    session_key: SessionKey,
    key: Arc<Keypair>,
    peer: Arc<Peer>,
    peer_list: Arc<RwLock<PeerList>>,
    transports: Arc<RwLock<HashMap<TransportType, Sender<EndpointSendMessage>>>>,
    is_recv_data: bool,
    is_relay_data: bool,
    is_stabled: Arc<Mutex<bool>>,
}

impl Session {
    async fn listen(
        &self,
        receiver: Receiver<SessionSendMessage>,
        endpoint_recv: Receiver<EndpointStreamMessage>,
    ) {
        let _ = future::try_zip(
            self.listen_outside(receiver),
            self.listen_endpoint(endpoint_recv),
        )
        .await;

        self.peer_list
            .write()
            .await
            .peer_remove(&self.remote_peer_id);

        if *self.is_stabled.lock().await {
            self.peer_list
                .write()
                .await
                .stable_leave(&self.remote_peer_id);
            self.out_sender
                .send(ReceiveMessage::StableLeave(self.remote_peer_id))
                .await
                .expect("Session to Outside (Close)");
        }
        debug!("Session broke: {:?}", self.remote_peer_id);
    }

    async fn listen_outside(&self, receiver: Receiver<SessionSendMessage>) -> Result<()> {
        loop {
            match receiver.recv().await {
                Ok(SessionSendMessage::Bytes(from, to, bytes)) => {
                    debug!("Got outside data to remote: {:?}", bytes.len());
                    let e_data = self.session_key.encrypt(bytes);
                    let data = SessionType::Data(from, to, e_data).to_bytes();
                    self.endpoint_send
                        .send(EndpointStreamMessage::Bytes(data))
                        .await
                        .expect("Session to Endpoint (Data)");
                }
                Ok(SessionSendMessage::Close) => {
                    debug!("Got outside close it");
                    self.endpoint_send
                        .send(EndpointStreamMessage::Close)
                        .await
                        .expect("Session to Endpoint (Close)");
                    break;
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
                    debug!("Session recv stable result to: {:?}, {}", to, is_ok);
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
                Err(e) => break,
            }
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "outside recever over",
        ))
    }

    async fn listen_endpoint(&self, endpoint_recv: Receiver<EndpointStreamMessage>) -> Result<()> {
        loop {
            match endpoint_recv.recv().await {
                Ok(EndpointStreamMessage::Bytes(bytes)) => {
                    if bytes.len() < 1 {
                        continue;
                    }
                    let t_type_byte = bytes[0].clone();
                    let t = SessionType::from_bytes(bytes);
                    if t.is_err() {
                        debug!("Debug: Error Serialize SessionType type is {}", t_type_byte);
                        continue;
                    }

                    match t.unwrap() {
                        SessionType::Key(bytes) => {
                            debug!("session rebuild new encrypt key.");
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
                            let d_data = self.session_key.decrypt(e_data);
                            if d_data.is_ok() {
                                if to == self.my_peer_id {
                                    if self.is_recv_data || *self.is_stabled.lock().await {
                                        self.out_sender
                                            .send(ReceiveMessage::Data(from, d_data.unwrap()))
                                            .await
                                            .expect("Session to Outside (Data)");
                                    } else {
                                        debug!("Permissioned: not accept data.")
                                    }
                                } else {
                                    if self.is_relay_data {
                                        // Relay
                                        let peer_list_lock = self.peer_list.read().await;
                                        let sender = peer_list_lock.get(&to);
                                        if sender.is_some() {
                                            let sender = sender.unwrap();
                                            sender
                                                .send(SessionSendMessage::Bytes(
                                                    from,
                                                    to,
                                                    d_data.unwrap(),
                                                ))
                                                .await
                                                .expect("Session to Session (Relay)");
                                        }
                                    } else {
                                        debug!("Permissioned: not accept relay.")
                                    }
                                }
                            }
                        }
                        SessionType::DHT(peers) => {
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
                Ok(EndpointStreamMessage::Close) => break,
                Err(_) => break,
            }
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "transport recever over",
        ))
    }
}

/// bytes[0] is type, bytes[1..] is data.
enum SessionType {
    /// type is 1u8.
    Key(Vec<u8>),
    /// type is 2u8.
    Data(PeerId, PeerId, Vec<u8>),
    /// type is 3u8.
    DHT(DHT),
    /// type is 4u8.
    Hole(Hole),
    /// type is 5u8.
    HoleConnect,
    /// type is 6u8.
    Ping,
    /// type is 7u8.
    Pong,
    /// type is 8u8.
    StableConnect(PeerId, Vec<u8>),
    /// type is 9u8.
    StableResult(PeerId, bool, Vec<u8>),
}

impl SessionType {
    fn to_bytes(self) -> Vec<u8> {
        let mut bytes = vec![0u8];
        match self {
            SessionType::Key(mut data) => {
                bytes[0] = 1u8;
                bytes.append(&mut data);
            }
            SessionType::Data(p1_id, p2_id, mut data) => {
                bytes[0] = 2u8;
                bytes.append(&mut p1_id.to_bytes());
                bytes.append(&mut p2_id.to_bytes());
                bytes.append(&mut data);
            }
            SessionType::DHT(dht) => {
                bytes[0] = 3u8;
                bytes.append(&mut dht.to_bytes());
            }
            SessionType::Hole(hole) => {
                bytes[0] = 4u8;
                bytes.push(hole.to_byte());
            }
            SessionType::HoleConnect => {
                bytes[0] = 5u8;
            }
            SessionType::Ping => {
                bytes[0] = 6u8;
            }
            SessionType::Pong => {
                bytes[0] = 7u8;
            }
            SessionType::StableConnect(p_id, mut data) => {
                bytes[0] = 8u8;
                bytes.append(&mut p_id.to_bytes());
                bytes.append(&mut data);
            }
            SessionType::StableResult(p_id, ok, mut data) => {
                bytes[0] = 9u8;
                bytes.append(&mut p_id.to_bytes());
                bytes.push(if ok { 1u8 } else { 0u8 });
                bytes.append(&mut data);
            }
        }

        bytes
    }

    fn from_bytes(mut bytes: Vec<u8>) -> std::result::Result<Self, ()> {
        if bytes.len() < 1 {
            return Err(());
        }

        let t: Vec<u8> = bytes.drain(0..1).collect();
        match t[0] {
            1u8 => Ok(SessionType::Key(bytes)),
            2u8 => {
                if bytes.len() < PEER_ID_LENGTH + PEER_ID_LENGTH {
                    return Err(());
                }
                let p1 = PeerId::from_bytes(&bytes[0..PEER_ID_LENGTH])?;
                let p2 =
                    PeerId::from_bytes(&bytes[PEER_ID_LENGTH..PEER_ID_LENGTH + PEER_ID_LENGTH])?;
                let _ = bytes.drain(0..PEER_ID_LENGTH + PEER_ID_LENGTH);
                Ok(SessionType::Data(p1, p2, bytes))
            }
            3u8 => {
                let dht = DHT::from_bytes(&bytes)?;
                Ok(SessionType::DHT(dht))
            }
            4u8 => {
                if bytes.len() != 1 {
                    return Err(());
                }
                let hole = Hole::from_byte(bytes[0])?;
                Ok(SessionType::Hole(hole))
            }
            5u8 => Ok(SessionType::HoleConnect),
            6u8 => Ok(SessionType::Ping),
            7u8 => Ok(SessionType::Pong),
            8u8 => {
                if bytes.len() < PEER_ID_LENGTH {
                    return Err(());
                }
                let p = PeerId::from_bytes(&bytes[0..PEER_ID_LENGTH])?;
                let _ = bytes.drain(0..PEER_ID_LENGTH);
                Ok(SessionType::StableConnect(p, bytes))
            }
            9u8 => {
                if bytes.len() < PEER_ID_LENGTH + 1 {
                    return Err(());
                }
                let p = PeerId::from_bytes(&bytes[0..PEER_ID_LENGTH])?;
                let ok = bytes[PEER_ID_LENGTH] == 1u8;
                let _ = bytes.drain(0..PEER_ID_LENGTH + 1);
                Ok(SessionType::StableResult(p, ok, bytes))
            }
            _ => Err(()),
        }
    }
}

/// Rtemote Public Info, include local transport and public key bytes.
#[derive(Debug)]
pub struct RemotePublic(pub Keypair, pub Peer);

impl RemotePublic {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() < PEER_LENGTH {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "serialize remote public error",
            ));
        }
        let peer = Peer::from_bytes(&bytes[..PEER_LENGTH]).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::Other, "serialize remote public error")
        })?;
        let keypair = Keypair::from_bytes(&bytes[PEER_LENGTH..]).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::Other, "serialize remote public error")
        })?;

        Ok(Self(keypair, peer))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![];
        bytes.append(&mut self.1.to_bytes());
        bytes.append(&mut self.0.to_bytes());
        bytes
    }
}
