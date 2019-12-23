use async_std::task;
use chamomile::{new_channel, start, Config};

fn main() {
    task::block_on(async {
        let (out_send, out_recv) = new_channel();
        let send = start(out_send, Config::default("127.0.0.1:8000".parse().unwrap()))
            .await
            .unwrap();

        while let Some(message) = out_recv.recv().await {
            println!("recv: {:?}", message);
            send.send(message).await;
        }
    });
}
