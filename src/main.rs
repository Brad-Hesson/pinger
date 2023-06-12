use clap::Parser;

mod ping;
mod view;
mod ui;
mod gpu;

#[tokio::main]
async fn main() {
    // console_subscriber::init();
    match Args::parse().subcommand {
        Subcommand::Ping(args) => ping::main(args).await,
        Subcommand::View(args) => view::main(args).await,
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
    /// Graphically display the response data from a .ping file
    View(view::Args),
    Ui,
}
