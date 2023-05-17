use clap::Parser;
use ipnet::Ipv4Net;
use iprange::IpRange;
use std::{
    fmt::Write,
    io::SeekFrom,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
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
    for net_string in args.subnets {
        range.add(net_string.parse().unwrap());
    }

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .open(path_from_range(range.clone()).unwrap())
        .await
        .unwrap();

    let num_done = file.metadata().await.unwrap().len() / DATA_SIZE;
    file.seek(SeekFrom::Start(num_done * DATA_SIZE))
        .await
        .unwrap();
    let addrs = range
        .into_iter()
        .flat_map(|net| net.hosts())
        .skip(num_done as usize);

    let total_num_addrs = range.into_iter().flat_map(|net| net.hosts()).count();
    println!("{total_num_addrs} addresses to ping in total");
    println!("{num_done} addresses already in file");

    let state = Arc::new(State::new(total_num_addrs as u64, num_done));
    let client = Arc::new(surge_ping::Client::new(&surge_ping::Config::default()).unwrap());

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<JoinHandle<Option<Duration>>>();

    let file_writer_handle = tokio::spawn(file_writer(rx, BufWriter::new(file)));
    tokio::spawn(stats_printer(
        state.clone(),
        Duration::from_secs(args.update_interval),
    ));
    for addr in addrs {
        let mut pinger = client.pinger(addr.into(), 0.into()).await;
        pinger.timeout(Duration::from_secs(args.timeout));
        state.num_running.fetch_add(1, Ordering::Release);
        let handle = tokio::spawn(worker(pinger, state.clone()));
        tx.send(handle).unwrap();
        while state.num_running.load(Ordering::Acquire) >= args.num_concurrent {
            tokio::task::yield_now().await;
        }
    }
    drop(tx);
    file_writer_handle.await.unwrap();
    tokio::time::sleep(Duration::from_secs(args.update_interval)).await;
}

async fn file_writer(
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

async fn stats_printer(state: Arc<State>, interval: Duration) {
    let mut last_time = Instant::now();
    let mut last_value = state.num_done.load(Ordering::Acquire);
    loop {
        tokio::time::sleep(interval).await;
        let now = Instant::now();
        let done = state.num_done.load(Ordering::Acquire);
        let active = state.num_running.load(Ordering::Acquire);
        let perc_done = done as f64 / state.total as f64 * 100.;
        let rate = (done - last_value) as f64 / (now - last_time).as_secs_f64();
        last_time = now;
        last_value = done;
        println!("{perc_done:>7.3}% done | {rate:>9.2} p/s | {active:>6} active",);
    }
}

async fn worker(mut pinger: surge_ping::Pinger, state: Arc<State>) -> Option<Duration> {
    let reply = pinger.ping(0.into(), &[]).await;
    state.num_done.fetch_add(1, Ordering::Release);
    state.num_running.fetch_sub(1, Ordering::Release);
    reply.ok().map(|(_, dur)| dur)
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
    num_done: AtomicU64,
    num_running: AtomicUsize,
    total: u64,
}
impl State {
    fn new(total: u64, done: u64) -> Self {
        Self {
            num_done: AtomicU64::new(done),
            num_running: AtomicUsize::new(0),
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
