use actix::io::FramedWrite;
use actix::io::WriteHandler;
use actix::prelude::{Actor, Context};
use bytes::Bytes;
use quinn::SendStream;
use tokio::codec::BytesCodec;

pub(crate) struct WriterStream {
    data: Vec<Bytes>,
    write_stream: FramedWrite<SendStream, BytesCodec>,
}

impl WriterStream {
    pub fn new(data: Bytes, write_stream: FramedWrite<SendStream, BytesCodec>) -> Self {
        Self {
            data: vec![data],
            write_stream: write_stream,
        }
    }
}

impl WriteHandler<std::io::Error> for WriterStream {}

impl Actor for WriterStream {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        println!("write stream started to send");
        let bytes = self.data.pop().unwrap();
        self.write_stream.write(bytes);
        //self.write_stream.close();
        //self.stopping(ctx);
    }
}
