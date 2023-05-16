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
    let args = Args::parse();
    console_subscriber::init();

    let mut range = IpRange::<Ipv4Net>::new();
    for n in args.subnets {
        range.add(n.parse().unwrap());
    }
    let num_ips = range.into_iter().flat_map(|net| net.hosts()).count();
    println!("{num_ips} addresses to ping");
    let ips = range.into_iter().flat_map(|net| net.hosts());

    let state = Arc::new(State::new(num_ips));
    let client = Arc::new(surge_ping::Client::new(&surge_ping::Config::default()).unwrap());

    tokio::spawn(printer(
        state.clone(),
        Duration::from_secs(args.update_interval),
    ));
    for addr in ips {
        let mut pinger = client.pinger(addr.into(), 0.into()).await;
        pinger.timeout(Duration::from_secs(args.timeout));
        state.running.fetch_add(1, Ordering::Release);
        tokio::spawn(worker(pinger, state.clone()));
        while state.running.load(Ordering::Acquire) >= args.num_concurrent {
            tokio::task::yield_now().await;
        }
    }
    while state.running.load(Ordering::Acquire) > 0 {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(Duration::from_secs(args.update_interval)).await;
}

async fn printer(state: Arc<State>, interval: Duration) {
    let mut last_time = Instant::now();
    let mut last_value = 0;
    loop {
        let done = state.num_done.load(Ordering::Acquire);
        let replied = state.num_replied.load(Ordering::Acquire);
        let active = state.running.load(Ordering::Acquire);
        let perc_done = done as f64 / state.total as f64 * 100.;
        let perc_replied = replied as f64 / done as f64 * 100.;
        let now = Instant::now();
        let rate = (done - last_value) as f64 / (now - last_time).as_secs_f64();
        last_time = now;
        last_value = done;
        println!(
            "{perc_done:.3}% done | {perc_replied:.3}% replied | {rate:.2} p/s | {active} active",
        );
        tokio::time::sleep(interval).await;
    }
}

async fn worker(mut pinger: surge_ping::Pinger, state: Arc<State>) -> Option<Duration> {
    let reply = pinger.ping(0.into(), &[]).await;
    let dur = match reply {
        Ok((_, dur)) => Some(dur),
        Err(_) => None,
    };
    if dur.is_some() {
        state.num_replied.fetch_add(1, Ordering::Release);
    }
    state.num_done.fetch_add(1, Ordering::Release);
    state.running.fetch_sub(1, Ordering::Release);
    dur
}

struct State {
    num_done: AtomicUsize,
    num_replied: AtomicUsize,
    running: AtomicUsize,
    total: usize,
}
impl State {
    fn new(total: usize) -> Self {
        Self {
            num_done: AtomicUsize::new(0),
            num_replied: AtomicUsize::new(0),
            running: AtomicUsize::new(0),
            total,
        }
    }
}

#[derive(Debug, Parser)]
struct Args {
    #[arg(default_value_t = 100_000, short, long)]
    /// The maximuim number of concurrent pings
    num_concurrent: usize,
    #[arg(default_value_t = 1, short, long)]
    /// The interval for printings stats in seconds
    update_interval: u64,
    #[arg(default_value_t = 2, short, long)]
    /// The ping timeout in seconds
    timeout: u64,
    #[arg(default_values_t = vec!["142.244.0.0/16".to_string(), "129.128.0.0/16".to_string()], short, long)]
    /// The subnets to ping
    subnets: Vec<String>,
}
