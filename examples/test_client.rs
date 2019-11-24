use chamomile::{start, Config, Message, new_channel};
use async_std::task;

fn main() {
    task::block_on(async {
        let (out_send, out_recv) = new_channel();
        let send = start(out_send, Config::default("127.0.0.1:8001".parse().unwrap())).await.unwrap();

        send.send(Message::Data(vec![1, 2, 3, 4])).await;

        while let Some(message) = out_recv.recv().await {
            println!("recv: {:?}", message);
        }
    });
}
