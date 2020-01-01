use async_std::io::Result;
use async_std::sync::{channel, Receiver, Sender};
use async_trait::async_trait;
use serde_derive::{Deserialize, Serialize};
use std::fmt::{Debug, Formatter, Result as FmtResult};
use std::net::SocketAddr;

mod rtp;
mod tcp;
mod udp;
mod udt;

use crate::core::peer::{Peer, PeerId};

/// max task capacity for udp to handle.
pub const MAX_MESSAGE_CAPACITY: usize = 1024;

#[derive(Debug, Copy, Clone, Hash, Deserialize, Serialize)]
pub enum TransportType {
    UDP, // 0u8
    TCP, // 1u8
    RTP, // 2u8
    UDT, // 3u8
}

impl TransportType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "udp" => TransportType::UDP,
            "tcp" => TransportType::TCP,
            "rtp" => TransportType::RTP,
            "udt" => TransportType::UDT,
            _ => TransportType::UDP,
        }
    }
}

/// Message Type for transport and outside.
pub enum EndpointMessage {
    Connect(SocketAddr, Vec<u8>), // server to transport
    Disconnect(SocketAddr),       // server to transport
    PreConnected(
        SocketAddr,
        Receiver<StreamMessage>,
        Sender<StreamMessage>,
        bool,
    ), // transport to server
    Connected(PeerId, Sender<StreamMessage>, Peer, Vec<u8>), // session to server
    Close(PeerId),                // session to server
}

/// StreamMessage use in out server and stream in channel.
pub enum StreamMessage {
    Ok(Vec<u8>),
    Close,
    Bytes(Vec<u8>),
}

impl Debug for EndpointMessage {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        match self {
            EndpointMessage::Connect(ref addr, _) => {
                write!(f, "Endpoint start connect: {:?}", addr)
            }
            EndpointMessage::Disconnect(ref addr) => write!(f, "Endpoint disconnected: {:?}", addr),
            EndpointMessage::PreConnected(ref addr, _, _, _) => {
                write!(f, "Endpoint pre-connected: {:?}", addr)
            }
            EndpointMessage::Connected(ref addr, _, _, _) => {
                write!(f, "Endpoint connected: {:?}", addr)
            }
            EndpointMessage::Close(ref addr) => write!(f, "Endpoint losed: {:?}", addr),
        }
    }
}

impl Debug for StreamMessage {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        match self {
            StreamMessage::Ok(_bytes) => write!(f, "Stream is ok."),
            StreamMessage::Close => write!(f, "Stream need close."),
            StreamMessage::Bytes(ref bytes) => write!(f, "Stream Bytes: {:?}.", bytes),
        }
    }
}

pub fn new_channel() -> (Sender<EndpointMessage>, Receiver<EndpointMessage>) {
    channel(MAX_MESSAGE_CAPACITY)
}

pub fn new_stream_channel() -> (Sender<StreamMessage>, Receiver<StreamMessage>) {
    channel(MAX_MESSAGE_CAPACITY)
}

/// Transports trait, all transport protocol will implement this.
#[async_trait]
pub trait Endpoint: Send {
    /// Init and run a Endpoint object.
    /// You need send a bind-socketaddr and received message's addr,
    /// and return the endpoint's sender addr.
    async fn start(
        bind_addr: SocketAddr,
        send_channel: Sender<EndpointMessage>,
    ) -> Result<Sender<EndpointMessage>>;
}

pub async fn start(
    transport: &TransportType,
    addr: &SocketAddr,
    sender: Sender<EndpointMessage>,
) -> Result<Sender<EndpointMessage>> {
    match transport {
        &TransportType::UDP => udp::UdpEndpoint::start(addr.clone(), sender).await,
        &TransportType::TCP => tcp::TcpEndpoint::start(addr.clone(), sender).await,
        _ => panic!("Not suppert, waiting"),
    }
}
