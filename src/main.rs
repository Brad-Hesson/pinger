use clap::Parser;
use tracing_chrome::ChromeLayerBuilder;
use tracing_subscriber::prelude::*;

mod gpu;
mod ping;
mod ping_map;
mod ui;
mod wgpu_ext;

#[tokio::main]
async fn main() {
    let (chrome_layer, _guard) = ChromeLayerBuilder::new()
        .include_args(true)
        .file("trace.json")
        .build();
    let fmt = tracing_subscriber::registry();
    fmt.with(chrome_layer).init();
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
