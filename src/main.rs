use futures::{stream, StreamExt};
use std::net::IpAddr;

#[tokio::main]
async fn main() {
    // console_subscriber::init();
    let client = surge_ping::Client::new(&surge_ping::Config::default()).unwrap();
    stream::iter(0..u32::MAX)
        .for_each_concurrent(256 * 256, |i| {
            let client = &client;
            async move {
                let addr = IpAddr::V4(i.into());
                let mut pinger = client.pinger(addr, 0.into()).await;
                let reply = pinger.ping(0.into(), &[]).await;
                if let Ok(reply) = reply {
                    println!("{addr:<15} : {:?}", reply.1)
                }
            }
        })
        .await;
}
