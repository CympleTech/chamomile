use async_std::task;
use chamomile::{new_channel, start, Config, Message};

fn main() {
    task::block_on(async {
        let (out_send, out_recv) = new_channel();
        let send = start(out_send, Config::default("127.0.0.1:8001".parse().unwrap()))
            .await
            .unwrap();

        //send.send(Message::Connect(vec![1, 2, 3, 4])).await;
        println!("start connect to test");
        send.send(Message::Connect("127.0.0.1:8000".parse().unwrap()))
            .await;
        println!("started connect to test");

        while let Some(message) = out_recv.recv().await {
            println!("recv: {:?}", message);
        }
    });
}
