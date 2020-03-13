use async_std::io::Result;
use async_std::sync::{channel, Receiver, Sender};
use async_trait::async_trait;
use serde_derive::{Deserialize, Serialize};
use std::net::SocketAddr;

mod message;
mod rtp;
mod tcp;
//mod udp;
mod udt;

pub use message::{EndpointIncomingMessage, EndpointSendMessage, EndpointStreamMessage};

use crate::primitives::MAX_MESSAGE_CAPACITY;

/// Transports types support by Endpoint.
#[derive(Debug, Copy, Clone, Hash, Deserialize, Serialize)]
pub enum TransportType {
    UDP, // 0u8
    TCP, // 1u8
    RTP, // 2u8
    UDT, // 3u8
}

impl TransportType {
    /// transports from parse from str.
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

/// new a channel for send EndpointSendMessage.
pub fn new_endpoint_send_channel() -> (Sender<EndpointSendMessage>, Receiver<EndpointSendMessage>) {
    channel(MAX_MESSAGE_CAPACITY)
}

/// new a channel for receive EndpointIncomingMessage.
pub fn new_endpoint_recv_channel() -> (
    Sender<EndpointIncomingMessage>,
    Receiver<EndpointIncomingMessage>,
) {
    channel(MAX_MESSAGE_CAPACITY)
}

/// new a channel for EndpointStreamMessage.
pub fn new_endpoint_stream_channel() -> (
    Sender<EndpointStreamMessage>,
    Receiver<EndpointStreamMessage>,
) {
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
        send: Sender<EndpointIncomingMessage>,
        recv: Receiver<EndpointSendMessage>,
    ) -> Result<()>;
}

/// main function. start the endpoint listening.
pub async fn start(
    transport: &TransportType,
    addr: SocketAddr,
) -> Result<(
    Sender<EndpointSendMessage>,
    Receiver<EndpointIncomingMessage>,
)> {
    let (send_send, send_recv) = new_endpoint_send_channel();
    let (recv_send, recv_recv) = new_endpoint_recv_channel();

    match transport {
        //&TransportType::UDP => udp::UdpEndpoint::start(addr, recv_send, send_recv).await?,
        &TransportType::TCP => tcp::TcpEndpoint::start(addr, recv_send, send_recv).await?,
        _ => panic!("Not suppert, waiting"),
    }

    Ok((send_send, recv_recv))
}
