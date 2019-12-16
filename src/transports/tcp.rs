use async_std::net::{TcpListener, TcpStream};
use async_std::io::{self, Result};
use async_std::prelude::*;
use async_std::sync::{Sender, Receiver, channel};
use async_std::sync::{Arc, Mutex};
use async_std::task;
use async_std::stream::StreamExt;
use async_trait::async_trait;
use std::net::SocketAddr;
use std::collections::HashMap;
use async_std::io::BufReader;

use futures::select;
use futures::FutureExt;
//use rand::{RngCore, thread_rng};


use crate::PeerId;
use super::{Endpoint, EndpointMessage, new_channel, MAX_MESSAGE_CAPACITY};

/// TCP Endpoint.
pub struct TcpEndpoint {
    peer_id: PeerId,
    //streams: HashMap<SocketAddr, (PeerId, Arc<TcpStream>)>
}

#[async_trait]
impl Endpoint for TcpEndpoint {
    /// Init and run a UdpEndpoint object.
    /// You need send a socketaddr str and udp send message's addr,
    /// and receiver outside message addr.
    async fn start(
        socket_addr: SocketAddr,
        peer_id: PeerId,
        out_send: Sender<EndpointMessage>
    ) -> Result<Sender<EndpointMessage>> {
        let (send, recv) = new_channel();
        let endpoint = TcpEndpoint { peer_id };

        let m1 = Arc::new(Mutex::new(endpoint));
        let m2 = m1.clone();

        // TCP listen
        task::spawn(run_listen(socket_addr, out_send.clone(), m1));

        // TCP listen from outside
        task::spawn(run_self_recv(recv, out_send, m2));

        Ok(send)
    }
}

async fn run_listen(
    socket_addr: SocketAddr,
    out_send: Sender<EndpointMessage>,
    endpoint: Arc<Mutex<TcpEndpoint>>,
) -> Result<()> {
    let listener = TcpListener::bind(socket_addr).await?;
    let mut incoming = listener.incoming();

    while let Some(stream) = incoming.next().await {
        task::spawn(process_stream(stream?, out_send.clone()));
    }

    drop(incoming);
    drop(listener);
    Ok(())
}

async fn run_self_recv(
    recv: Receiver<EndpointMessage>,
    out_send: Sender<EndpointMessage>,
    endpoint: Arc<Mutex<TcpEndpoint>>,
) -> Result<()> {
    while let Some(m) = recv.recv().await {
        match m {
            EndpointMessage::Connect(addr) => {
                let stream = TcpStream::connect(addr).await?;
                task::spawn(process_stream(stream, out_send.clone()));
            },
            _ => {}
        }
    }
    Ok(())
}

async fn process_stream(stream: TcpStream, sender: Sender<EndpointMessage>) -> Result<()> {
    let addr = stream.peer_addr()?;
    let (reader, mut writer) = (&stream, &stream);

    let (self_sender, self_receiver) = channel(MAX_MESSAGE_CAPACITY);
    let (out_sender, out_receiver) = channel(MAX_MESSAGE_CAPACITY);
    sender.send(EndpointMessage::Connected(addr.clone(), out_receiver, self_sender)).await;
    let peer_id: PeerId = Default::default();

    let reader = BufReader::new(reader);

    let mut lines_from_stream = futures::StreamExt::fuse(reader.lines());
    let mut lines_from_recv = futures::StreamExt::fuse(self_receiver);

    loop {
        select! {
            line = lines_from_stream.next().fuse() => match line {
                Some(line) => {
                    let value = line?;
                    out_sender.send(value.as_bytes().to_vec()).await;
                },
                None => break,
            },
            line = lines_from_recv.next().fuse() => match line {
                Some(line) => {
                    writer.write_all(&line[..]).await;
                    writer.write_all(b"\n").await;
                },
                None => break,
            },
        }
    }

    Ok(())
}
