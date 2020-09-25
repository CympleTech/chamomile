use smol::{
    channel::{Receiver, Sender},
    io::{BufReader, Result},
    lock::Mutex,
    net::{TcpListener, TcpStream},
    prelude::*,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::ops::Div;
use std::sync::Arc;

use super::message::{EndpointIncomingMessage, EndpointSendMessage, EndpointStreamMessage};
use super::new_endpoint_stream_channel;

/// TCP Endpoint.
pub struct TcpEndpoint {
    streams: HashMap<SocketAddr, Sender<EndpointStreamMessage>>,
}

impl TcpEndpoint {
    /// Init and run a UdpEndpoint object.
    /// You need send a socketaddr str and udp send message's addr,
    /// and receiver outside message addr.
    pub async fn start(
        bind_addr: SocketAddr,
        send: Sender<EndpointIncomingMessage>,
        recv: Receiver<EndpointSendMessage>,
    ) -> Result<()> {
        let endpoint = TcpEndpoint {
            streams: HashMap::new(),
        };

        let m1 = Arc::new(Mutex::new(endpoint));
        let m2 = m1.clone();

        // TCP listen
        smol::spawn(run_listen(bind_addr, send.clone(), m1)).detach();

        // TCP listen from outside
        smol::spawn(run_self_recv(recv, send, m2)).detach();

        Ok(())
    }
}

async fn run_listen(
    socket_addr: SocketAddr,
    out_send: Sender<EndpointIncomingMessage>,
    endpoint: Arc<Mutex<TcpEndpoint>>,
) -> Result<()> {
    let listener = TcpListener::bind(socket_addr)
        .await
        .expect("TCP listen failure!");
    let mut incoming = listener.incoming();

    while let Some(Ok(stream)) = incoming.next().await {
        smol::spawn(process_stream(
            stream,
            out_send.clone(),
            endpoint.clone(),
            false,
        ))
        .detach();
    }

    drop(incoming);
    drop(listener);
    Ok(())
}

async fn run_self_recv(
    recv: Receiver<EndpointSendMessage>,
    out_send: Sender<EndpointIncomingMessage>,
    endpoint: Arc<Mutex<TcpEndpoint>>,
) -> Result<()> {
    while let Ok(m) = recv.recv().await {
        match m {
            EndpointSendMessage::Connect(addr, is_stable, bytes) => {
                if let Ok(mut stream) = TcpStream::connect(addr).await {
                    let len = bytes.len() as u32;
                    let _ = stream.write(&(len.to_be_bytes())).await;
                    let _ = stream.write_all(&bytes[..]).await;
                    smol::spawn(process_stream(
                        stream,
                        out_send.clone(),
                        endpoint.clone(),
                        is_stable,
                    ))
                    .detach();
                }
            }
            EndpointSendMessage::Close(ref addr) => {
                if let Some(sender) = endpoint.lock().await.streams.remove(addr) {
                    sender
                        .send(EndpointStreamMessage::Close)
                        .await
                        .expect("Endpoint to Server (Close)");
                }
            }
        }
    }

    Ok(())
}

async fn process_stream(
    stream: TcpStream,
    sender: Sender<EndpointIncomingMessage>,
    endpoint: Arc<Mutex<TcpEndpoint>>,
    is_stable: bool,
) -> Result<()> {
    let addr = stream.peer_addr()?;
    let mut reader = stream.clone();
    let mut writer = stream;

    let (self_sender, self_receiver) = new_endpoint_stream_channel();
    let (out_sender, out_receiver) = new_endpoint_stream_channel();

    endpoint
        .lock()
        .await
        .streams
        .entry(addr)
        .and_modify(|s| *s = self_sender.clone())
        .or_insert(self_sender.clone());

    sender
        .send(EndpointIncomingMessage(
            addr,
            out_receiver,
            self_sender,
            is_stable,
        ))
        .await
        .expect("Endpoint to Server (Incoming)");

    smol::spawn(async move {
        loop {
            match self_receiver.recv().await {
                Ok(msg) => match msg {
                    EndpointStreamMessage::Bytes(bytes) => {
                        let len = bytes.len() as u32;
                        let _ = writer.write(&(len.to_be_bytes())).await;
                        let _ = writer.write_all(&bytes[..]).await;
                    }
                    EndpointStreamMessage::Close => break,
                },
                Err(_) => break,
            }
        }
    })
    .detach();

    let mut read_len = [0u8; 4];
    let mut received: usize = 0;

    loop {
        match reader.read(&mut read_len).await {
            Ok(size) => {
                if size == 0 {
                    // when close or better when many Ok(0)
                    out_sender
                        .send(EndpointStreamMessage::Close)
                        .await
                        .expect("Endpoint to Session (Size Close)");
                    break;
                }

                let len: usize = u32::from_be_bytes(read_len) as usize;
                let mut read_bytes = vec![0u8; len];
                while let Ok(bytes_size) = reader.read(&mut read_bytes).await {
                    received += bytes_size;
                    if received > len {
                        break;
                    }

                    if received != len {
                        continue;
                    }

                    out_sender
                        .send(EndpointStreamMessage::Bytes(read_bytes.clone()))
                        .await
                        .expect("Endpoint to Session (Bytes)");

                    break;
                }
                read_len = [0u8; 4];
                received = 0;
            }
            Err(_e) => {
                out_sender
                    .send(EndpointStreamMessage::Close)
                    .await
                    .expect("Endpoint to Session (Close)");
                break;
            }
        }
    }

    //stream.close().await?;
    endpoint.lock().await.streams.remove(&addr);
    drop(out_sender);
    debug!("close stream: {}", addr);

    Ok(())
}
