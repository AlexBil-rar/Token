// ghost-node/src/main.rs

mod cli;
mod node_runner;
mod genesis;
mod ws_server;
mod bootstrap;
mod peer_discovery;
mod gossip;

use clap::Parser;
use tracing::info;
use cli::Cli;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_max_level(cli.log_level())
        .with_target(false)
        .init();

    info!("Starting GhostLedger node v{}", env!("CARGO_PKG_VERSION"));
    info!("Port: {}", cli.port);
    info!("Data dir: {}", cli.data_dir);

    if let Err(e) = node_runner::run(cli).await {
        eprintln!("Fatal error: {}", e);
        std::process::exit(1);
    }
}