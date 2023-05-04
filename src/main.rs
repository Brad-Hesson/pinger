use clap::Parser;
use futures::{
    future::{join_all, select},
    stream, StreamExt,
};
use itertools::Itertools;
use std::{
    net::IpAddr,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[tokio::main]
async fn main() {
    console_subscriber::init();
    let args = Args::parse();
    let n = args.num_threads;

    let state = Arc::new(State::new());

    let max = u32::MAX;
    let part = max / n;
    let workers_h = join_all(
        (0..n)
            .map(|i| i * part)
            .chain(Some(max))
            .tuple_windows()
            .map(|(a, b)| tokio::spawn(worker(a..b, state.clone()))),
    );
    let printer_h = tokio::spawn(printer(state));

    select(workers_h, printer_h).await;
}

async fn printer(state: Arc<State>) {
    let mut last_time = Instant::now();
    let mut last_value = 0;
    loop {
        let done = state.num_done.load(Ordering::Relaxed);
        // let replied = state.num_replied.load(Ordering::Relaxed);
        let percentage = done as f64 / u32::MAX as f64 * 100.;
        let now = Instant::now();
        let rate = (done - last_value) as f64 / (now - last_time).as_secs_f64();
        last_time = now;
        last_value = done;
        println!("{percentage:.3}%  @  {rate:.2} p/s",);
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}

async fn worker(i: impl IntoIterator<Item = u32>, state: Arc<State>) {
    let client = surge_ping::Client::new(&surge_ping::Config::default()).unwrap();
    stream::iter(i)
        .for_each_concurrent(100_000, |i| {
            let client = &client;
            let state = &state;
            async move {
                let addr = IpAddr::V4(i.into());
                let mut pinger = client.pinger(addr, 0.into()).await;
                pinger.timeout(Duration::from_secs(5));
                let reply = pinger.ping(0.into(), &[]).await;
                if let Ok(_) = reply {
                    state.num_replied.fetch_add(1, Ordering::Relaxed);
                }
                state.num_done.fetch_add(1, Ordering::Relaxed);
            }
        })
        .await;
}

struct State {
    num_done: AtomicUsize,
    num_replied: AtomicUsize,
}
impl State {
    fn new() -> Self {
        Self {
            num_done: AtomicUsize::new(0),
            num_replied: AtomicUsize::new(0),
        }
    }
}

#[derive(Debug, Parser)]
struct Args {
    #[arg(default_value_t = 1, short, long)]
    num_threads: u32,
}
