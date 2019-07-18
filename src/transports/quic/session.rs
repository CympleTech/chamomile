use actix::io::FramedWrite;
use actix::prelude::*;
use multiaddr::Multiaddr;
use quinn::{Connection, ConnectionDriver, IncomingStreams};
use tokio::codec::BytesCodec;
use tokio::codec::FramedRead;

use crate::core::peer_id::PeerID;
use crate::core::server::ServerActor;
use crate::core::session::{SessionClose, SessionCreate, SessionOpen, SessionReceive, SessionSend};

use super::super::TransportType;
use super::reader_stream::ReaderStream;
use super::writer_stream::WriterStream;

pub(crate) struct NewStream(pub quinn::NewStream);

impl Message for NewStream {
    type Result = ();
}

pub(crate) struct QuicSessionActor {
    self_peer_id: PeerID,
    server_addr: Addr<ServerActor>,
    drivers: Vec<ConnectionDriver>,
    incoming: Vec<IncomingStreams>,
    conn: Connection,
}

impl QuicSessionActor {
    pub fn new(
        self_peer_id: PeerID,
        server_addr: Addr<ServerActor>,
        driver: ConnectionDriver,
        conn: Connection,
        incoming: IncomingStreams,
    ) -> Self {
        let drivers = vec![driver];
        let incoming = vec![incoming];

        Self {
            self_peer_id,
            server_addr,
            drivers,
            conn,
            incoming,
        }
    }

    pub fn remote_addr(&self) -> Multiaddr {
        TransportType::QUIC.to_multiaddr(&self.conn.remote_address())
    }

    pub fn heartbeat(&self, ctx: &mut Context<Self>) {}
}

impl Actor for QuicSessionActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        let self_addr = ctx.address();
        let driver = self.drivers.pop().unwrap();

        tokio::runtime::current_thread::spawn(driver.map_err(move |e| {
            println!("DEBUG: Error in connection Driver: {:?}", e);
            self_addr.do_send(SessionClose(Default::default()));
        }));

        let comingers =
            self.incoming
                .pop()
                .unwrap()
                .map_err(|_| ())
                .for_each(|stream| match stream {
                    quinn::NewStream::Bi(w, r) => {
                        let reader = r.read_to_end(1000000).and_then(|(_stream, bytes)| {
                            println!("DEBUG: Receive : {:?}", bytes);
                            Ok(())
                        });

                        tokio::runtime::current_thread::spawn(
                            reader.map_err(|e| println!("read failure: {}", e)),
                        );

                        let data = vec![1, 2, 3, 4];

                        let writer = tokio::io::write_all(w, data)
                            .map_err(|_| println!("write error"))
                            .and_then(|(mut w, _)| {
                                w.poll_finish().map_err(|e| println!("write finish error"));
                                Ok(())
                            });

                        tokio::runtime::current_thread::spawn(writer);

                        Ok(())
                    }
                    quinn::NewStream::Uni(r) => {
                        let reader = r.read_to_end(1000000).and_then(|(_stream, bytes)| {
                            println!("DEBUG: Receive : {:?}", bytes);
                            Ok(())
                        });

                        tokio::runtime::current_thread::spawn(
                            reader.map_err(|e| println!("read failure: {}", e)),
                        );

                        Ok(())
                    }
                });

        tokio::runtime::current_thread::spawn(comingers);

        // ctx.add_message_stream(
        //     self.incoming
        //         .pop()
        //         .unwrap()
        //         .map_err(|_| ())
        //         .map(|stream| NewStream(stream)),
        // );

        self.server_addr.do_send(SessionOpen(
            self.self_peer_id.clone(),
            self.remote_addr(),
            ctx.address().recipient::<SessionSend>(),
            vec![],
        ));

        self.heartbeat(ctx);
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        self.server_addr.do_send(SessionClose(Default::default()));

        Running::Stop
    }
}

impl Handler<NewStream> for QuicSessionActor {
    type Result = ();

    fn handle(&mut self, msg: NewStream, _ctx: &mut Context<Self>) -> Self::Result {
        println!("DEBUG: Receive new stream, wait reading...");
        let server_addr = self.server_addr.clone();
        let peer_id = self.self_peer_id.clone(); // TODO remote peer_id

        match msg.0 {
            quinn::NewStream::Bi(_w, r) => {
                ReaderStream::create(move |r_ctx| {
                    ReaderStream::add_stream(FramedRead::new(r, BytesCodec::new()), r_ctx);
                    ReaderStream::new(peer_id, server_addr)
                });
            }
            quinn::NewStream::Uni(r) => {
                //let mut buf = bytes::BytesMut::new();
                //let reader = r.poll_read(&mut buf);
                let reader = r.read_to_end(1000000).and_then(|(_stream, bytes)| {
                    println!("DEBUG: Receive : {:?}", bytes);
                    Ok(())
                });

                tokio::runtime::current_thread::spawn(
                    reader.map_err(|e| println!("read failure: {}", e)),
                );

                // Start SessionActor

                // ReaderStream::create(move |r_ctx| {
                //     ReaderStream::add_stream(FramedRead::new(r, BytesCodec::new()), r_ctx);
                //     ReaderStream::new(peer_id, server_addr)
                // });
            }
        };
    }
}

impl Handler<SessionSend> for QuicSessionActor {
    type Result = ();

    fn handle(&mut self, msg: SessionSend, _ctx: &mut Context<Self>) {
        let data = msg.1;
        println!("receive need send data: {:?}", data);
        let writer = self
            .conn
            .open_uni()
            .map_err(|_e| {
                println!("send bytes to connect failure!");
            })
            .and_then(move |w| {
                println!("start sending...");
                tokio::io::write_all(w, data).map_err(|e| println!("write error"))
            })
            .and_then(|(mut w, _)| {
                w.poll_finish().map_err(|e| println!("write finish error"));
                Ok(())
            });

        // match send.poll_write(&data[..]) {
        //     Ok(futures::Async::Ready(t)) => {
        //         println!("send ok: {}", t);
        //         Ok(())
        //     }
        //     Ok(futures::Async::NotReady) => {
        //         println!("send not ok");
        //         Ok(())
        //     }
        //     Err(e) => {
        //         println!("send failure: {}", e);
        //         Ok(())
        //     }
        // }
        // WriterStream::create(move |ctx| {
        //     WriterStream::new(data.into(), FramedWrite::new(send, BytesCodec::new(), ctx))
        // });

        //});

        tokio::runtime::current_thread::spawn(writer);
    }
}

impl Handler<SessionClose> for QuicSessionActor {
    type Result = ();

    fn handle(&mut self, _msg: SessionClose, ctx: &mut Context<Self>) {
        println!("start close session");
        self.stopping(ctx);
    }
}
