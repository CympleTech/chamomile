use serde::{Deserialize, Serialize};
use smol::{
    channel::{self, Receiver, Sender},
    io::Result,
};
use std::net::SocketAddr;

mod message;
mod rtp;
mod tcp;
//mod udp;
mod udt;

use chamomile_types::types::TransportType;

pub use message::{EndpointIncomingMessage, EndpointSendMessage, EndpointStreamMessage};

/// new a channel for send EndpointSendMessage.
pub fn new_endpoint_send_channel() -> (Sender<EndpointSendMessage>, Receiver<EndpointSendMessage>) {
    channel::unbounded()
}

/// new a channel for receive EndpointIncomingMessage.
pub fn new_endpoint_recv_channel() -> (
    Sender<EndpointIncomingMessage>,
    Receiver<EndpointIncomingMessage>,
) {
    channel::unbounded()
}

/// new a channel for EndpointStreamMessage.
pub fn new_endpoint_stream_channel() -> (
    Sender<EndpointStreamMessage>,
    Receiver<EndpointStreamMessage>,
) {
    channel::unbounded()
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
