use anyhow::Result;
use tokio::{net::{TcpListener, TcpStream}, io::AsyncWriteExt};
use std::{fs::File, io::{BufRead,BufReader}, time::{SystemTime, UNIX_EPOCH}};
use tracing::{info, error};
use tracing_subscriber::EnvFilter;

const PORT: u16 = 38291;
const BOOT_TXT: &str = "bootstrap/seedlist.txt";
const BOOT_RT: &str = "bootstrap/seedlist.runtime";

fn load_seeds() -> Vec<String> {
    let mut out = Vec::new();
    for p in [BOOT_TXT, BOOT_RT] {
        if let Ok(f) = File::open(p) {
            for line in BufReader::new(f).lines().flatten() {
                let ln = line.trim().to_string();
                if !ln.is_empty() && !ln.starts_with(#) {
                    out.push(ln);
                }
            }
        }
    }
    out
}

async fn handle_client(mut sock: TcpStream) -> Result<()> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let seeds: Vec<String> = load_seeds().into_iter().take(8).collect();
    let banner = serde_json::json!({
        "banner": "Irium rust node skeleton",
        "time": now,
        "seeds": seeds,
    }).to_string() + "\n";
    sock.write_all(banner.as_bytes()).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env()
            .add_directive("info".parse().unwrap()))
        .init();

    let addr = format!("0.0.0.0:{}", PORT);
    let listener = TcpListener::bind(&addr).await?;
    info!("iriumd skeleton listening on {}", addr);

    loop {
        match listener.accept().await {
            Ok((sock, peer)) => {
                info!("connection from {}", peer);
                tokio::spawn(async move {
                    if let Err(e) = handle_client(sock).await {
                        eprintln!("handler error: {e}");
                    }
                });
            }
            Err(e) => {
                error!("accept error: {}", e);
            }
        }
    }
}
