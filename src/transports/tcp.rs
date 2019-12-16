use async_std::{
    prelude::*,
    io::{BufReader, Result},
    net::{TcpListener, TcpStream},
    sync::{Sender, Receiver, Arc, Mutex},
    task
};
use async_trait::async_trait;
use std::net::SocketAddr;
use std::collections::HashMap;
use futures::{FutureExt, select};

use crate::PeerId;
use super::{Endpoint, EndpointMessage, StreamMessage, new_channel, new_stream_channel};

/// TCP Endpoint.
pub struct TcpEndpoint {
    _peer_id: PeerId,
    streams: HashMap<SocketAddr, Sender<StreamMessage>>
}

#[async_trait]
impl Endpoint for TcpEndpoint {
    /// Init and run a UdpEndpoint object.
    /// You need send a socketaddr str and udp send message's addr,
    /// and receiver outside message addr.
    async fn start(
        socket_addr: SocketAddr,
        _peer_id: PeerId,
        out_send: Sender<EndpointMessage>
    ) -> Result<Sender<EndpointMessage>> {
        let (send, recv) = new_channel();
        let endpoint = TcpEndpoint { _peer_id, streams: HashMap::new() };

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
        task::spawn(process_stream(stream?, out_send.clone(), endpoint.clone()));
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
                task::spawn(process_stream(stream, out_send.clone(), endpoint.clone()));
            },
            EndpointMessage::Disconnect(ref addr) => {
                let mut endpoint = endpoint.lock().await;
                if let Some(sender) = endpoint.streams.remove(addr) {
                    sender.send(StreamMessage::Close).await;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

async fn process_stream(stream: TcpStream, sender: Sender<EndpointMessage>, endpoint: Arc<Mutex<TcpEndpoint>>) -> Result<()> {
    let addr = stream.peer_addr()?;
    let (reader, mut writer) = (&stream, &stream);

    let (self_sender, self_receiver) = new_stream_channel();
    let (out_sender, out_receiver) = new_stream_channel();

    let peer_id: PeerId = Default::default();
    endpoint.lock().await.streams.entry(addr).and_modify(|s| *s = self_sender.clone()).or_insert(self_sender.clone());
    sender.send(EndpointMessage::Connected(peer_id, out_receiver, self_sender)).await;

    let reader = BufReader::new(reader);

    let mut lines_from_stream = futures::StreamExt::fuse(reader.lines());
    let mut lines_from_recv = futures::StreamExt::fuse(self_receiver);

    loop {
        select! {
            line = lines_from_stream.next().fuse() => match line {
                Some(line) => {
                    let value = line?;
                    out_sender.send(StreamMessage::Bytes(value.as_bytes().to_vec())).await;
                },
                None => break,
            },
            line = lines_from_recv.next().fuse() => match line {
                Some(line) => {
                    match line {
                        StreamMessage::Bytes(bytes) => {
                            writer.write_all(&bytes[..]).await?;
                            writer.write_all(b"\n").await?;
                        }
                        StreamMessage::Close => {
                            return Ok(())
                        }
                    }
                },
                None => break,
            },
        }
    }

    Ok(())
}
