use async_std::{
    io::{BufReader, Result},
    net::{TcpListener, TcpStream},
    prelude::*,
    sync::{Arc, Mutex, Receiver, Sender},
    task,
};
use async_trait::async_trait;
use futures::{select, FutureExt};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::ops::Div;

use super::message::{EndpointIncomingMessage, EndpointSendMessage, EndpointStreamMessage};
use super::{new_endpoint_stream_channel, Endpoint};

/// TCP Endpoint.
pub struct TcpEndpoint {
    streams: HashMap<SocketAddr, Sender<EndpointStreamMessage>>,
}

#[async_trait]
impl Endpoint for TcpEndpoint {
    /// Init and run a UdpEndpoint object.
    /// You need send a socketaddr str and udp send message's addr,
    /// and receiver outside message addr.
    async fn start(
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
        task::spawn(run_listen(bind_addr, send.clone(), m1));

        // TCP listen from outside
        task::spawn(run_self_recv(recv, send, m2));

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
        task::spawn(process_stream(
            stream,
            out_send.clone(),
            endpoint.clone(),
            false,
        ));
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
    while let Some(m) = recv.recv().await {
        match m {
            EndpointSendMessage::Connect(addr, bytes) => {
                if let Ok(mut stream) = TcpStream::connect(addr).await {
                    let len = bytes.len() as u32;
                    let _ = stream.write(&(len.to_be_bytes())).await;
                    let _ = stream.write_all(&bytes[..]).await;
                    task::spawn(process_stream(
                        stream,
                        out_send.clone(),
                        endpoint.clone(),
                        true,
                    ));
                }
            }
            EndpointSendMessage::Close(ref addr) => {
                let mut endpoint = endpoint.lock().await;
                if let Some(sender) = endpoint.streams.remove(addr) {
                    sender.send(EndpointStreamMessage::Close).await;
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
    is_ok: bool,
) -> Result<()> {
    let addr = stream.peer_addr()?;

    let (mut reader, mut writer) = &mut (&stream, &stream);

    //let stream = Arc::new(stream);
    //let mut reader = BufReader::new(&*stream);
    //let writer = Arc::clone(&stream);

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
            is_ok,
        ))
        .await;

    let mut read_len = [0u8; 4];

    loop {
        select! {
            msg = reader.read(&mut read_len).fuse() => match msg {
                Ok(size) => {
                    if size == 0 {
                        // when close or better when many Ok(0)
                        out_sender.send(EndpointStreamMessage::Close).await;
                        break;
                    }

                    let len: usize = u32::from_be_bytes(read_len) as usize;
                    let mut received: usize = 0;
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
                            .await;
                        break;
                    }
                    read_len = [0u8; 4];
                    received = 0;
                }
                Err(e) => {
                    out_sender.send(EndpointStreamMessage::Close).await;
                    break;
                }
            },
            msg = self_receiver.recv().fuse() => match msg {
                Some(msg) => {
                    match msg {
                        EndpointStreamMessage::Bytes(bytes) => {
                            let len = bytes.len() as u32;
                            writer.write(&(len.to_be_bytes())).await?;
                            writer.write_all(&bytes[..]).await?;
                        }
                        EndpointStreamMessage::Close => break,
                    }
                },
                None => break,
            }
        }
    }

    endpoint.lock().await.streams.remove(&addr);
    drop(self_receiver);
    drop(out_sender);
    println!("close stream: {}", addr);

    Ok(())
}
