use clap::Parser;

mod gpu;
mod ping;
mod ping_map;
mod ui;

#[tokio::main]
async fn main() {
    // console_subscriber::init();
    match Args::parse().subcommand {
        Subcommand::Ping(args) => ping::main(args).await,
        Subcommand::Ui => ui::main().await,
    }
}
#[derive(Parser, Debug)]
struct Args {
    #[command(subcommand)]
    subcommand: Subcommand,
}

#[derive(Debug, clap::Subcommand)]
enum Subcommand {
    /// Ping a provided range of addresses and save the response durations to a file
    Ping(ping::Args),
    Ui,
}
