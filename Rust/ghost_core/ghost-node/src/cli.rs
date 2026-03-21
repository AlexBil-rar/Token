// ghost-node/src/cli.rs

use clap::Parser;
use tracing::Level;

#[derive(Parser, Debug)]
#[command(name = "ghostledger")]
#[command(about = "GhostLedger node — feeless anonymous DAG ledger")]
#[command(version)]
pub struct Cli {
    #[arg(short, long, default_value = "9000")]
    pub port: u16,

    #[arg(long, num_args = 0..)]
    pub peers: Vec<String>,

    #[arg(long, default_value = "data")]
    pub data_dir: String,

    #[arg(long, default_value = "false")]
    pub genesis: bool,

    #[arg(long)]
    pub genesis_address: Option<String>,

    #[arg(long, default_value = "info")]
    pub log: String,
}

impl Cli {
    pub fn log_level(&self) -> Level {
        match self.log.to_lowercase().as_str() {
            "error" => Level::ERROR,
            "warn"  => Level::WARN,
            "debug" => Level::DEBUG,
            "trace" => Level::TRACE,
            _       => Level::INFO,
        }
    }

    pub fn snapshot_path(&self) -> String {
        format!("{}/node_{}.json", self.data_dir, self.port)
    }
}