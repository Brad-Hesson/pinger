use clap::Parser;
use ipnet::Ipv4Net;
use iprange::IpRange;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[tokio::main]
async fn main() {
    console_subscriber::init();
    let mut range = IpRange::<Ipv4Net>::new();
    range.add("0.0.0.0/0".parse().unwrap());
    // range.add("142.244.0.0/16".parse().unwrap());
    // range.add("129.128.0.0/16".parse().unwrap());

    let ips = range.into_iter().flat_map(|net| net.hosts());
    let state = Arc::new(State::new());
    let client = Arc::new(surge_ping::Client::new(&surge_ping::Config::default()).unwrap());

    tokio::spawn(printer(state.clone()));
    for addr in ips {
        let mut pinger = client.pinger(addr.into(), 0.into()).await;
        pinger.timeout(Duration::from_secs(5));
        state.running.fetch_add(1, Ordering::Release);
        tokio::spawn(worker(pinger, state.clone()));
        while state.running.load(Ordering::Acquire) >= 100_000 {
            tokio::task::yield_now().await;
        }
    }
}

async fn printer(state: Arc<State>) {
    let mut last_time = Instant::now();
    let mut last_value = 0;
    loop {
        let done = state.num_done.load(Ordering::Acquire);
        let replied = state.num_replied.load(Ordering::Acquire);
        let active = state.running.load(Ordering::Acquire);
        let perc_done = done as f64 / u32::MAX as f64 * 100.;
        let perc_replied = replied as f64 / done as f64 * 100.;
        let now = Instant::now();
        let rate = (done - last_value) as f64 / (now - last_time).as_secs_f64();
        last_time = now;
        last_value = done;
        println!(
            "{perc_done:.3}% done | {perc_replied:.3}% replied | {rate:.2} p/s | {active} active",
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn worker(mut pinger: surge_ping::Pinger, state: Arc<State>) {
    let reply = pinger.ping(0.into(), &[]).await;
    if let Ok(_) = reply {
        state.num_replied.fetch_add(1, Ordering::Release);
    }
    state.num_done.fetch_add(1, Ordering::Release);
    state.running.fetch_sub(1, Ordering::Release);
}

struct State {
    num_done: AtomicUsize,
    num_replied: AtomicUsize,
    running: AtomicUsize,
}
impl State {
    fn new() -> Self {
        Self {
            num_done: AtomicUsize::new(0),
            num_replied: AtomicUsize::new(0),
            running: AtomicUsize::new(0),
        }
    }
}

#[derive(Debug, Parser)]
struct Args {
    #[arg(default_value_t = 1, short, long)]
    num_threads: u32,
}
