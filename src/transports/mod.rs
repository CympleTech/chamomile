use async_std::io::Result;
use async_std::sync::{channel, Sender, Receiver};
use async_trait::async_trait;
use std::net::SocketAddr;
use std::fmt::{Debug, Formatter, Result as FmtResult};

mod udp;
mod tcp;
mod rtp;
mod udt;

use crate::PeerId;

/// max task capacity for udp to handle.
pub const MAX_MESSAGE_CAPACITY: usize = 1024;

/// Message Type for transport and outside.
pub enum EndpointMessage {
    Connect(SocketAddr),
    Disconnect(SocketAddr),
    Connected(PeerId, Receiver<StreamMessage>, Sender<StreamMessage>),
}

/// StreamMessage use in out server and stream in channel.
pub enum StreamMessage {
    Close,
    Bytes(Vec<u8>)
}

impl Debug for EndpointMessage {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        match self {
            EndpointMessage::Connect(ref addr) => write!(f, "Endpoint start connect: {:?}", addr),
            EndpointMessage::Connected(ref addr, _, _) => write!(f, "Endpoint connected: {:?}", addr),
            EndpointMessage::Disconnect(ref addr) => write!(f, "Endpoint disconnected: {:?}", addr),
        }
    }
}

impl Debug for StreamMessage {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        match self {
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
        peer_id: PeerId,
        send_channel: Sender<EndpointMessage>,
    ) -> Result<Sender<EndpointMessage>>;
}

pub use udp::UdpEndpoint;
pub use tcp::TcpEndpoint;
//TODO pub use rtp::RtpEndpoint;
//TODO pub use udt::UdtEndpoint;
