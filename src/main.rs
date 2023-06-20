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
    let args = Args::parse();
    if args.trace {
        let (chrome_layer, _guard) = ChromeLayerBuilder::new()
            .include_args(true)
            .file("trace.json")
            .build();
        tracing_subscriber::registry().with(chrome_layer).init();
    }
    match args.subcommand {
        Subcommand::Ping(args) => ping::main(args).await,
        Subcommand::Gui => ui::main().await,
    }
}
#[derive(Parser, Debug)]
struct Args {
    #[command(subcommand)]
    subcommand: Subcommand,
    /// Generate a trace file
    #[arg(short, long)]
    trace: bool,
}

#[derive(Debug, clap::Subcommand)]
enum Subcommand {
    /// Ping a provided range of addresses and save the response durations to a file
    Ping(ping::Args),
    /// Open a GUI for viewing ping files
    Gui,
}
