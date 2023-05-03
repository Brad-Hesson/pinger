use std::net::{IpAddr, Ipv4Addr};
use futures::{stream, StreamExt};

#[tokio::main]
async fn main() {
    // console_subscriber::init();
    let client = surge_ping::Client::new(&surge_ping::Config::default()).unwrap();
    stream::iter(0..u32::MAX)
        .for_each_concurrent(256*256, |i| {
            let client = &client;
            async move {
                let addr = IpAddr::V4(i.into());
                let mut pinger = client.pinger(addr, 0.into()).await;
                let reply = pinger.ping(0.into(), &[]).await;
                match reply {
                    Ok(reply) => println!("{addr} : {:?}", reply.1),
                    Err(_) => {}
                }
            }
        })
        .await;
}
