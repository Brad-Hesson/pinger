use futures::{stream, StreamExt, future::join_all};
use itertools::Itertools;
use std::{net::IpAddr, time::Duration};

#[tokio::main]
async fn main() {
    console_subscriber::init();
    let n = 4;
    let part = u32::MAX / n;
    let mut handles = vec![];
    let mut num_done = 0;
    for (a, b) in (0..n)
        .map(|i| i * part)
        .chain(Some(u32::MAX))
        .tuple_windows()
    {
        let h = tokio::spawn(task(a..b));
        handles.push(h);
    }
    join_all(handles).await;
}

async fn task(i: impl IntoIterator<Item = u32>) {
    let client = surge_ping::Client::new(&surge_ping::Config::default()).unwrap();
    stream::iter(i)
        .for_each_concurrent(100_000, |i| {
            let client = &client;
            async move {
                let addr = IpAddr::V4(i.into());
                let mut pinger = client.pinger(addr, 0.into()).await;
                pinger.timeout(Duration::from_secs(5));
                let reply = pinger.ping(0.into(), &[]).await;
                if let Ok(reply) = reply {
                    println!("{addr:<15} : {:?}", reply.1)
                }
            }
        })
        .await;
}
