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

pub async fn main(args: Args) {
    // Construct the collection of subnets from the cli arg
    let mut range = IpRange::<Ipv4Net>::new();
    for net_string in args.subnets {
        range.add(net_string.parse().unwrap());
    }

    // Open (or create) the file that does/will contain the data
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .open(path_from_range(range.clone()).unwrap())
        .await
        .unwrap();

    // Compute the number of completed pings in the file by dividing by the size of each entry. If an
    // entry is only half written, we round down (using integer division) so that we overwrite it in
    // this execution.
    let num_done = file.metadata().await.unwrap().len() / DATA_SIZE;

    // Seek forward in the file to the entry after the last complete entry.  If the file was just
    // created then this entry will just be the start of the file.
    file.seek(SeekFrom::Start(num_done * DATA_SIZE))
        .await
        .unwrap();

    // Construct an iterator that will yield the remaining addresses to ping.
    let addrs = range
        .into_iter()
        .flat_map(|net| net.hosts())
        .skip(num_done as usize);

    // Count the total number of addresses in the specified network range. Print the total number
    // and remaining number of addresses to screen.
    let total_num_addrs = range.into_iter().flat_map(|net| net.hosts()).count();
    println!("{total_num_addrs} addresses to ping in total");
    println!("{num_done} addresses already in the file");

    // Construct the shared state struct and the pinger client struct.
    let state = Arc::new(State::new(total_num_addrs as u64, num_done));
    let client = Arc::new(surge_ping::Client::new(&surge_ping::Config::default()).unwrap());

    // Construct the channel that will be used to send ping results to the file writer.
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<JoinHandle<Option<Duration>>>();

    // Spawn the file writer task, which is given the reciever end of the channel and the file handle
    // wrapped in a BufWriter to speed up writes.
    let file_writer_handle = tokio::spawn(file_writer(rx, BufWriter::new(file)));

    // Spawn the stats printer task, which is given a reference to the shared state and the update
    // interval from the cli arg.
    let stats_printer_handle = tokio::spawn(stats_printer(
        state.clone(),
        Duration::from_secs(args.update_interval),
    ));

    // For every address in the iterator of remaining addresses:
    for addr in addrs {
        // Construct a pinger
        let mut pinger = client.pinger(addr.into(), 0.into()).await;
        // Set the timout based on the cli arg
        pinger.timeout(Duration::from_secs(args.timeout));
        // Add 1 to the running count
        state.num_running.fetch_add(1, Ordering::Release);
        // Spawn the worker
        let handle = tokio::spawn(ping_worker(pinger, state.clone()));
        // Send the worker handle to the file writer
        tx.send(handle).unwrap();
        // Wait until the running count drops below the max threshold given in the cli arg
        while state.num_running.load(Ordering::Acquire) >= args.num_concurrent {
            tokio::task::yield_now().await;
        }
    }

    // Now that all the pings have been sent, drop the channel sender in order to signal to the file writer
    // that no more handles will be added to the message queue and it can exit once it is done.
    drop(tx);

    // Wait for the file writer and stats_printer to complete before exiting.
    file_writer_handle.await.unwrap();
    stats_printer_handle.await.unwrap();
}

async fn file_writer(
    mut rx: UnboundedReceiver<JoinHandle<Option<Duration>>>,
    mut file: BufWriter<File>,
) {
    // As long as there is another ping worker handle in the message queue:
    // Wait for the ping to either return or timeout.
    while let Some(handle) = rx.recv().await {
        // Get the duration of the ping, or use -1 for a timeout.
        let num = match handle.await.unwrap() {
            Some(dur) => dur.as_secs_f32(),
            None => -1.,
        };
        // Write the number to the file in binary.
        file.write_f32(num).await.unwrap();
    }
    // Once completed, flush the buffer to the file.
    file.flush().await.unwrap();
}

async fn stats_printer(state: Arc<State>, interval: Duration) {
    // Set the baseline time and progress value
    let mut last_time = Instant::now();
    let mut last_value = state.num_done.load(Ordering::Acquire);
    loop {
        // Wait for the interval specified in the cli args
        tokio::time::sleep(interval).await;
        // Get the current time, progress value, and running count
        let now = Instant::now();
        let done = state.num_done.load(Ordering::Acquire);
        let active = state.num_running.load(Ordering::Acquire);
        // Calculate the progress percentage and the ping rate
        let perc_done = done as f64 / state.total as f64 * 100.;
        let rate = (done - last_value) as f64 / (now - last_time).as_secs_f64();
        // Set the baselines to the current values
        last_time = now;
        last_value = done;
        // Print the results
        println!("{perc_done:>7.3}% done | {rate:>9.2} p/s | {active:>6} active",);
        // If the number completed is equal to the total number of addrs, then we must
        // be done, so we exit the loop
        if done == state.total {
            break;
        }
    }
}

async fn ping_worker(mut pinger: surge_ping::Pinger, state: Arc<State>) -> Option<Duration> {
    // Start the ping and await its return.
    let reply = pinger.ping(0.into(), &[]).await;
    // Now that the ping has returned, add 1 to num_done and subtract 1 from the running count
    state.num_done.fetch_add(1, Ordering::Release);
    state.num_running.fetch_sub(1, Ordering::Release);
    // Return an optional duration based on if the ping timed out or returned successfully.
    reply.ok().map(|(_, dur)| dur)
}

fn path_from_range(mut range: IpRange<Ipv4Net>) -> Result<PathBuf, std::fmt::Error> {
    range.simplify();
    let mut name = String::new();
    for net in range.iter() {
        write!(name, "{}-{}_", net.network(), net.prefix_len())?;
    }
    name.pop();
    write!(name, ".ping")?;
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

#[derive(Debug, clap::Args)]
pub struct Args {
    /// The maximuim number of concurrent pings
    #[arg(default_value_t = 100_000, short, long)]
    num_concurrent: usize,
    /// The interval for printings stats in seconds
    #[arg(default_value_t = 1, short, long)]
    update_interval: u64,
    /// The ping timeout in seconds
    #[arg(default_value_t = 2, short, long)]
    timeout: u64,
    /// The subnets to ping
    #[arg(default_values_t = vec!["142.244.0.0/16".to_string(), "129.128.0.0/16".to_string()], short, long)]
    subnets: Vec<String>,
}
