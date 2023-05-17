use clap::Parser;
use ipnet::Ipv4Net;
use iprange::IpRange;
use std::{
    fmt::Write,
    io::SeekFrom,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::{
    fs::{File, OpenOptions},
    io::{AsyncSeekExt, AsyncWriteExt, BufWriter},
    sync::mpsc::UnboundedReceiver,
    task::JoinHandle,
};

const DATA_SIZE: u64 = std::mem::size_of::<f32>() as u64;

#[tokio::main]
async fn main() {
    let args = Args::parse();
    console_subscriber::init();

    let mut range = IpRange::<Ipv4Net>::new();
    for n in args.subnets {
        range.add(n.parse().unwrap());
    }
    let fname = path_from_range(range.clone()).unwrap();
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .open(fname)
        .await
        .unwrap();
    let num_done = file.metadata().await.unwrap().len() / DATA_SIZE;
    file.seek(SeekFrom::Start(num_done * DATA_SIZE))
        .await
        .unwrap();
    let ips = range
        .into_iter()
        .flat_map(|net| net.hosts())
        .skip(num_done as usize);
    let num_ips = range.into_iter().flat_map(|net| net.hosts()).count();
    println!("{num_ips} addresses to ping");

    let state = Arc::new(State::new(num_ips));
    state
        .num_done
        .fetch_add(num_done as usize, Ordering::Release);
    let client = Arc::new(surge_ping::Client::new(&surge_ping::Config::default()).unwrap());

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<JoinHandle<Option<Duration>>>();
    let reciever_h = tokio::spawn(reciever(rx, BufWriter::new(file)));
    tokio::spawn(printer(
        state.clone(),
        Duration::from_secs(args.update_interval),
    ));
    for addr in ips {
        let mut pinger = client.pinger(addr.into(), 0.into()).await;
        pinger.timeout(Duration::from_secs(args.timeout));
        state.running.fetch_add(1, Ordering::Release);
        let handle = tokio::spawn(worker(pinger, state.clone()));
        tx.send(handle).unwrap();
        while state.running.load(Ordering::Acquire) >= args.num_concurrent {
            tokio::task::yield_now().await;
        }
    }
    drop(tx);
    reciever_h.await.unwrap();
    tokio::time::sleep(Duration::from_secs(args.update_interval)).await;
}

async fn reciever(
    mut rx: UnboundedReceiver<JoinHandle<Option<Duration>>>,
    mut file: BufWriter<File>,
) {
    while let Some(handle) = rx.recv().await {
        match handle.await.unwrap() {
            Some(dur) => file.write_f32(dur.as_secs_f32()).await.unwrap(),
            None => file.write_f32(-1.).await.unwrap(),
        }
    }
    file.flush().await.unwrap();
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

fn path_from_range(mut range: IpRange<Ipv4Net>) -> Result<PathBuf, std::fmt::Error> {
    range.simplify();
    let mut name = String::new();
    for net in range.iter() {
        write!(name, "{}-{}_", net.network(), net.prefix_len())?;
    }
    name.pop();
    write!(name, ".bin")?;
    Ok(name.into())
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
