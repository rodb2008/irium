use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::fs;
use tracing::{info, warn};
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Parser)]
#[command(name="iriumd", version, author="iriumlabs", about="Irium node daemon")]
struct Cli {
    /// Increase verbosity (-v, -vv)
    #[arg(short, long, global=true, action=clap::ArgAction::Count)]
    verbose: u8,
    #[command(subcommand)]
    cmd: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the node services (placeholder)
    Start {
        #[arg(long, default_value="configs")]
        config_dir: String,
    },
    /// Load and verify the genesis header JSON
    CheckGenesis {
        #[arg(long, default_value="configs/genesis.header.json")]
        genesis: String,
    },
    /// P2P handshake placeholder
    P2pHandshake {
        #[arg(long, default_value="127.0.0.1:38291")]
        peer: String,
    },
}

#[derive(Deserialize, Debug)]
struct GenesisHeader {
    chain_id: String,
    genesis_time: String,
    merkle_root: String,
    nonce: u64,
    // Add additional fields if your header needs them
}

fn init_tracing(verbosity: u8) {
    let level = match verbosity { 0 => "info", 1 => "debug", _ => "trace" };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    fmt().with_env_filter(filter).with_target(false).init();
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    match cli.cmd {
        Commands::Start { config_dir } => {
            info!(%config_dir, "starting iriumd (placeholder)");
            // Placeholder: future work will start services and spawn listeners
            println!("iriumd start (config_dir={})", config_dir);
            Ok(())
        }
        Commands::CheckGenesis { genesis } => {
            let raw = fs::read_to_string(&genesis)
                .with_context(|| format!("reading {}", genesis))?;
            let parsed: GenesisHeader = serde_json::from_str(&raw)
                .with_context(|| "parsing genesis header JSON")?;
            info!(?parsed, "loaded genesis header");
            // TODO: implement concrete verification (hash/merkle) in next patch
            println!("Genesis loaded: {:?}", parsed);
            Ok(())
        }
        Commands::P2pHandshake { peer } => {
            warn!(%peer, "P2P handshake stub (no network I/O performed)");
            println!("P2P handshake stub for peer {}", peer);
            Ok(())
        }
    }
}
