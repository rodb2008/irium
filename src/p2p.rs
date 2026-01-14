use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use chrono::Utc;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{tcp::OwnedWriteHalf, TcpListener, TcpStream};
use tokio::sync::Mutex;

use crate::block::Block;
use crate::chain::ChainState;
use crate::storage;
use crate::mempool::MempoolManager;
use crate::network::{PeerDirectory, PeerRecord};
use crate::protocol::{
    BlockPayload, EmptyPayload, GetBlocksPayload, GetDataPayload, GetHeadersPayload,
    HandshakePayload, HeadersPayload, InvPayload, MempoolPayload, Message, MessageType,
    PeersPayload, PingPayload, RelayAddressPayload, TxPayload, MAX_BLOCKS_PER_REQUEST, MAX_HEADERS_PER_REQUEST, MAX_MESSAGE_SIZE,
};
use crate::reputation::ReputationManager;
use crate::sybil::{SybilChallenge, SybilProof, SybilResistantHandshake};
use crate::tx::decode_full_tx;
use rand_core::{OsRng, RngCore};
use hex;
use serde_json::json;

/// Minimal P2P node skeleton: accepts incoming connections and can
/// broadcast raw block bytes to all connected peers.
const DEFAULT_MAX_PEERS: usize = 100;
const MAX_MSGS_PER_SEC: u32 = 200;

fn sync_cooldown() -> Duration {
    let secs = std::env::var("IRIUM_P2P_SYNC_COOLDOWN_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(10);
    Duration::from_secs(secs.max(1).min(300))
}

async fn sync_request_allowed(
    sync_requests: &Arc<Mutex<HashMap<IpAddr, Instant>>>,
    ip: IpAddr,
) -> bool {
    let mut guard = sync_requests.lock().await;
    let now = Instant::now();
    if let Some(last) = guard.get(&ip) {
        if now.duration_since(*last) < sync_cooldown() {
            return false;
        }
    }
    guard.insert(ip, now);
    true
}

async fn sync_block_request_allowed(
    block_requests: &Arc<Mutex<HashMap<IpAddr, Instant>>>,
    ip: IpAddr,
) -> bool {
    let mut guard = block_requests.lock().await;
    let now = Instant::now();
    if let Some(last) = guard.get(&ip) {
        if now.duration_since(*last) < sync_cooldown() {
            return false;
        }
    }
    guard.insert(ip, now);
    true
}

fn getblocks_grace() -> Duration {
    let secs = std::env::var("IRIUM_P2P_GETBLOCKS_GRACE_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(8);
    Duration::from_secs(secs.max(2).min(60))
}

fn max_peers() -> usize {
    let val = std::env::var("IRIUM_P2P_MAX_PEERS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(DEFAULT_MAX_PEERS);
    val.clamp(10, 500)
}

async fn getblocks_request_allowed(
    requests: &Arc<Mutex<HashMap<IpAddr, (Vec<u8>, u32, Instant)>>>,
    ip: IpAddr,
    start_hash: &[u8],
    count: u32,
) -> bool {
    let grace = getblocks_grace();
    let now = Instant::now();
    let mut guard = requests.lock().await;
    if let Some((last_hash, last_count, last_ts)) = guard.get(&ip) {
        if *last_count == count
            && last_hash.as_slice() == start_hash
            && now.duration_since(*last_ts) < grace
        {
            return false;
        }
    }
    guard.insert(ip, (start_hash.to_vec(), count, now));
    true
}

#[derive(Clone)]
pub struct P2PNode {
    bind_addr: SocketAddr,
    peers: Arc<Mutex<Vec<Arc<Mutex<OwnedWriteHalf>>>>>,
    peers_directory: Arc<Mutex<PeerDirectory>>,
    connected: Arc<Mutex<HashSet<SocketAddr>>>,
    reputation: Arc<Mutex<ReputationManager>>,
    accept_log: Arc<Mutex<HashMap<IpAddr, Instant>>>,
    sync_requests: Arc<Mutex<HashMap<IpAddr, Instant>>>,
    block_requests: Arc<Mutex<HashMap<IpAddr, Instant>>>,
    getblocks_seen: Arc<Mutex<HashMap<IpAddr, Instant>>>,
    getblocks_last: Arc<Mutex<HashMap<IpAddr, (Vec<u8>, u32, Instant)>>>,
    self_ips: Arc<Mutex<HashSet<IpAddr>>>,
    dynamic_bans: Arc<StdMutex<HashMap<IpAddr, Instant>>>,
    chain: Option<Arc<StdMutex<ChainState>>>,
    mempool: Option<Arc<StdMutex<MempoolManager>>>,
    agent: String,
    relay_address: Option<String>,
    node_id: Vec<u8>,
    banned_ips: Arc<HashSet<IpAddr>>,
}

impl P2PNode {
    fn ts() -> String {
        Utc::now().format("%H:%M:%S").to_string()
    }

    fn json_log_enabled() -> bool {
        static FLAG: OnceLock<bool> = OnceLock::new();
        *FLAG.get_or_init(|| {
            std::env::var("IRIUM_JSON_LOG")
                .ok()
                .map(|v| v == "1" || v.to_lowercase() == "true")
                .unwrap_or(false)
        })
    }

    fn log_event(level: &str, category: &str, msg: impl AsRef<str>) {
        let icon = match category {
            "net" => "📡",
            "p2p" => "🔌",
            "chain" => "⛓️",
            "sync" => "🔁",
            "reputation" => "🛡️",
            "mempool" => "🧺",
            _ => "",
        };
        if Self::json_log_enabled() {
            let payload = json!({
                "ts": Self::ts(),
                "level": level,
                "cat": category,
                "icon": icon,
                "msg": msg.as_ref(),
            });
            if level == "error" {
                eprintln!("{}", payload);
            } else {
                println!("{}", payload);
            }
        } else {
            let tag = if icon.is_empty() {
                category.to_string()
            } else {
                format!("{} {}", icon, category)
            };
            let line = format!("[{}] [{}] {}", Self::ts(), tag, msg.as_ref());
            if level == "error" {
                eprintln!("{}", line);
            } else {
                println!("{}", line);
            }
        }
    }

    fn log(msg: impl AsRef<str>) {
        Self::log_event("info", "p2p", msg);
    }

    fn log_err(msg: impl AsRef<str>) {
        Self::log_event("error", "p2p", msg);
    }

    fn verbose_messages() -> bool {
        static FLAG: OnceLock<bool> = OnceLock::new();
        *FLAG.get_or_init(|| {
            std::env::var("IRIUM_VERBOSE_P2P")
                .ok()
                .map(|v| {
                    let v = v.to_lowercase();
                    !(v == "0" || v == "false" || v == "off")
                })
                .unwrap_or(true)
        })
    }

    fn ping_interval() -> Duration {
        let secs = std::env::var("IRIUM_P2P_PING_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(60);
        let bounded = secs.max(5).min(300);
        Duration::from_secs(bounded)
    }

    fn handshake_interval() -> Duration {
        let secs = std::env::var("IRIUM_P2P_HANDSHAKE_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(120);
        let bounded = secs.max(30).min(600);
        Duration::from_secs(bounded)
    }

    fn peer_timeout() -> Duration {
        let secs = std::env::var("IRIUM_P2P_PEER_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(180);
        let bounded = secs.max(30).min(600);
        Duration::from_secs(bounded)
    }

    fn is_soft_block_reject(reason: &str) -> bool {
        let msg = reason.to_lowercase();
        msg.contains("duplicate block") || msg.contains("orphan") || msg.contains("unknown parent")
    }

    fn is_duplicate_block(reason: &str) -> bool {
        reason.to_lowercase().contains("duplicate block")
    }

    fn is_banned(&self, ip: &IpAddr) -> bool {
        if self.banned_ips.contains(ip) {
            return true;
        }
        Self::is_banned_ip(ip, &self.banned_ips, &self.dynamic_bans)
    }

    fn is_banned_ip(
        ip: &IpAddr,
        static_bans: &HashSet<IpAddr>,
        dynamic_bans: &Arc<StdMutex<HashMap<IpAddr, Instant>>>,
    ) -> bool {
        if static_bans.contains(ip) {
            return true;
        }
        let mut guard = dynamic_bans.lock().unwrap();
        let expire = Duration::from_secs(600);
        if let Some(ts) = guard.get(ip) {
            if ts.elapsed() < expire {
                return true;
            }
            guard.remove(ip);
        }
        false
    }

    fn tip_hash(chain: &Option<Arc<StdMutex<ChainState>>>) -> [u8; 32] {
        if let Some(ref c) = chain {
            let guard = c.lock().unwrap();
            if let Some(last) = guard.chain.last() {
                return last.header.hash();
            }
        }
        [0u8; 32]
    }

    fn load_banned_ips() -> Arc<HashSet<IpAddr>> {
        let path = std::env::var("IRIUM_BANNED_LIST")
            .unwrap_or_else(|_| "bootstrap/banned_peers.txt".to_string());
        let mut ips = HashSet::new();
        let data = match fs::read_to_string(&path) {
            Ok(d) => d,
            Err(_) => return Arc::new(ips),
        };
        for line in data.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Ok(ip) = line.parse::<IpAddr>() {
                ips.insert(ip);
            }
        }
        Arc::new(ips)
    }

    fn sybil_difficulty() -> u8 {
        std::env::var("IRIUM_SYBIL_DIFFICULTY")
            .ok()
            .and_then(|v| v.parse::<u8>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(10)
    }

    fn load_or_create_node_id() -> Vec<u8> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
        let path = PathBuf::from(home).join(".irium/node_id");
        if let Ok(existing) = fs::read_to_string(&path) {
            if let Ok(bytes) = hex::decode(existing.trim()) {
                if bytes.len() == 32 {
                    return bytes;
                }
            }
        }
        let mut buf = [0u8; 32];
        OsRng.fill_bytes(&mut buf);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&path, hex::encode(buf));
        buf.to_vec()
    }

    pub fn new(
        bind_addr: SocketAddr,
        agent: String,
        chain: Option<Arc<StdMutex<ChainState>>>,
        mempool: Option<Arc<StdMutex<MempoolManager>>>,
        relay_address: Option<String>,
    ) -> Self {
        P2PNode {
            bind_addr,
            peers: Arc::new(Mutex::new(Vec::new())),
            peers_directory: Arc::new(Mutex::new(PeerDirectory::new())),
            connected: Arc::new(Mutex::new(HashSet::new())),
            reputation: Arc::new(Mutex::new(ReputationManager::new())),
            accept_log: Arc::new(Mutex::new(HashMap::new())),
            sync_requests: Arc::new(Mutex::new(HashMap::new())),
            block_requests: Arc::new(Mutex::new(HashMap::new())),
            getblocks_seen: Arc::new(Mutex::new(HashMap::new())),
            getblocks_last: Arc::new(Mutex::new(HashMap::new())),
            self_ips: Arc::new(Mutex::new(HashSet::new())),
            dynamic_bans: Arc::new(StdMutex::new(HashMap::new())),
            chain,
            mempool,
            agent,
            relay_address,
            node_id: Self::load_or_create_node_id(),
            banned_ips: Self::load_banned_ips(),
        }
    }

    /// Start listening for incoming peers. This is a basic skeleton and
    /// performs a basic sybil-resistant handshake before accepting peers.
    pub async fn start(&self) -> Result<(), String> {
        let listener = TcpListener::bind(self.bind_addr)
            .await
            .map_err(|e| e.to_string())?;
        Self::log(format!("P2P listening on {}", self.bind_addr));

        let peers_arc = self.peers.clone();
        let bind = self.bind_addr;
        let dir_arc = self.peers_directory.clone();
        let rep_arc = self.reputation.clone();
        let connected = self.connected.clone();
        let chain = self.chain.clone();
        let mempool = self.mempool.clone();
        let agent = self.agent.clone();
        let relay_address = self.relay_address.clone();
        let accept_log = self.accept_log.clone();
        let sync_requests = self.sync_requests.clone();
        let block_requests = self.block_requests.clone();
        let getblocks_seen = self.getblocks_seen.clone();
        let getblocks_last = self.getblocks_last.clone();
        let self_ips = self.self_ips.clone();
        let dynamic_bans = self.dynamic_bans.clone();
        let node_id = self.node_id.clone();
        let banned_ips = self.banned_ips.clone();

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, addr)) => {
                        let ip = addr.ip();
                        let dynamic_bans_check = dynamic_bans.clone();
                        if P2PNode::is_banned_ip(&ip, &banned_ips, &dynamic_bans_check) {
                            Self::log_err(format!("Rejecting inbound {}: banned", addr));
                            continue;
                        }
                        {
                            let guard = self_ips.lock().await;
                            if guard.contains(&ip) {
                                P2PNode::log_event(
                                    "warn",
                                    "net",
                                    format!("Rejecting inbound {}: self-connection", addr),
                                );
                                continue;
                            }
                        }
                        let mut log_guard = accept_log.lock().await;
                        if let Some(last) = log_guard.get(&ip) {
                            if last.elapsed() < Duration::from_millis(500) {
                                Self::log_err(format!("Rejecting inbound {}: rate limit", addr));
                                continue;
                            }
                        }
                        log_guard.insert(ip, Instant::now());
                        drop(log_guard);

                        let current = peers_arc.lock().await.len();
                        if current >= max_peers() {
                            Self::log_err(format!("Rejecting inbound {}: max peers reached", addr));
                            continue;
                        }
                        Self::log(format!("Incoming P2P connection from {}", addr));
                        let peers_inner = peers_arc.clone();
                        let connected_inner = connected.clone();
                        let dir = dir_arc.clone();
                        let rep = rep_arc.clone();
                        let chain_peer = chain.clone();
                        let mempool_peer = mempool.clone();
                        let agent_peer = agent.clone();
                        let relay_peer = relay_address.clone();
                        let node_id_peer = node_id.clone();
                        let self_ip_peer = self_ips.clone();
                        let sync_peer = sync_requests.clone();
                        let block_peer = block_requests.clone();
                        let getblocks_peer = getblocks_seen.clone();
                        let getblocks_last_peer = getblocks_last.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_incoming_with_sybil(
                                socket,
                                addr,
                                bind,
                                peers_inner,
                                connected_inner.clone(),
                                dir.clone(),
                                rep.clone(),
                                sync_peer,
                                block_peer,
                                getblocks_peer,
                                getblocks_last_peer,
                                self_ip_peer,
                                chain_peer,
                                mempool_peer,
                                agent_peer,
                                relay_peer,
                                node_id_peer,
                            )
                            .await
                            {
                                Self::log_err(format!("P2P handshake error from {}: {}", addr, e));
                            }
                        });
                    }
                    Err(e) => {
                        Self::log_err(format!("P2P accept error: {}", e));
                    }
                }
            }
        });

        Ok(())
    }

    /// Broadcast a raw serialized block to all currently known peers.
    pub async fn peer_count(&self) -> usize {
        self.peers.lock().await.len()
    }

    pub async fn peers_snapshot(&self) -> Vec<PeerRecord> {
        let dir = self.peers_directory.lock().await;
        let connected = self.connected.lock().await;
        dir.peers()
            .into_iter()
            .filter(|rec| {
                if let Some(addr) = Self::parse_multiaddr(&rec.multiaddr) {
                    connected.contains(&addr)
                } else {
                    false
                }
            })
            .collect()
    }

    /// Request peer lists from all connected peers.
    pub async fn request_peers(&self) -> Result<(), String> {
        let msg = EmptyPayload::to_message(MessageType::GetPeers)?;
        let bytes = msg.serialize();
        let mut guard = self.peers.lock().await;
        for socket in guard.iter_mut() {
            if let Err(e) = socket.lock().await.write_all(&bytes).await {
                return Err(format!("failed to send getpeers: {}", e));
            }
        }
        Ok(())
    }

    /// Force a refresh of the runtime seedlist based on current peer directory.
    pub async fn refresh_seedlist(&self) {
        let dir = self.peers_directory.lock().await;
        dir.refresh_seedlist_with_policy();
    }

    /// Parse a multiaddr like /ip4/1.2.3.4/tcp/38291 into a SocketAddr.
    fn parse_multiaddr(multiaddr: &str) -> Option<std::net::SocketAddr> {
        let parts: Vec<&str> = multiaddr.split('/')
            .filter(|s| !s.is_empty())
            .collect();
        if parts.len() < 4 {
            return None;
        }
        match parts[0] {
            "ip4" | "ip6" => {}
            _ => return None,
        }
        let ip: std::net::IpAddr = parts[1].parse().ok()?;
        if parts[2] != "tcp" {
            return None;
        }
        let port: u16 = parts[3].parse().ok()?;
        Some(std::net::SocketAddr::new(ip, port))
    }

    fn local_height_value(&self) -> u64 {
        local_height(&self.chain)
    }

    /// Opportunistically dial peers we have learned about from gossip.
    /// Opportunistically dial peers we have learned about from gossip.
    pub async fn connect_known_peers(&self, max_new: usize) {
        let current = self.peer_count().await;
        let mut added = 0usize;
        let peers = {
            let dir = self.peers_directory.lock().await;
            dir.peers()
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        let mut scanned = 0usize;

        for record in peers {
            scanned += 1;
            if scanned > 10 {
                break;
            }
            if current + added >= MAX_PEERS || added >= max_new {
                break;
            }
            // Skip peers we just connected to recently.
            if now > record.last_seen && (now - record.last_seen) < 30.0 {
                continue;
            }
            if let Some(addr) = Self::parse_multiaddr(&record.multiaddr) {
                // Skip connecting to ourselves or banned peers.
                if (addr.ip() == self.bind_addr.ip() && addr.port() == self.bind_addr.port())
                    || self.is_banned(&addr.ip())
                {
                    continue;
                }
                if self.is_self_ip(addr.ip()).await {
                    continue;
                }
                if self.is_ip_connected(addr.ip()).await {
                    continue;
                }
                if self
                    .connect_and_handshake(addr, self.local_height_value(), &self.agent)
                    .await
                    .is_ok()
                {
                    added += 1;
                }
            }
        }
    }


    pub async fn current_sybil_difficulty(&self) -> u8 {
        let base = Self::sybil_difficulty();
        let max = std::env::var("IRIUM_SYBIL_DIFFICULTY_MAX")
            .ok()
            .and_then(|v| v.parse::<u8>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(20);
        let banned = {
            let rep = self.reputation.lock().await;
            rep.banned_count() as u8
        };
        let adj = base.saturating_add(banned.min(5));
        adj.min(max)
    }

    pub fn node_id_hex(&self) -> String {
        hex::encode(&self.node_id)
    }

    pub async fn is_connected(&self, addr: &SocketAddr) -> bool {
        let guard = self.connected.lock().await;
        guard.contains(addr)
    }

    pub async fn is_ip_connected(&self, ip: IpAddr) -> bool {
        let guard = self.connected.lock().await;
        guard.iter().any(|addr| addr.ip() == ip)
    }

    pub async fn is_self_ip(&self, ip: IpAddr) -> bool {
        let guard = self.self_ips.lock().await;
        guard.contains(&ip)
    }

    pub async fn broadcast_block(&self, block_bytes: &[u8]) -> Result<(), String> {
        let msg = BlockPayload {
            block_data: block_bytes.to_vec(),
        }
        .to_message();
        let serialized = msg.serialize();

        let mut guard = self.peers.lock().await;
        for socket in guard.iter_mut() {
            let _ = socket.lock().await.write_all(&serialized).await;
        }
        Ok(())
    }

    /// Broadcast a raw serialized transaction to all connected peers.
    pub async fn broadcast_tx(&self, tx_bytes: &[u8]) -> Result<(), String> {
        let msg = TxPayload {
            tx_data: tx_bytes.to_vec(),
        }
        .to_message();
        let serialized = msg.serialize();

        let mut guard = self.peers.lock().await;
        for socket in guard.iter_mut() {
            if let Err(e) = socket.lock().await.write_all(&serialized).await {
                eprintln!("Failed to send tx to peer: {}", e);
            }
        }
        Ok(())
    }

    /// Broadcast an INV for given txids.
    pub async fn broadcast_inv(&self, txids: Vec<String>) -> Result<(), String> {
        if txids.is_empty() {
            return Ok(());
        }
        let msg = InvPayload { txids }.to_message()?;
        let serialized = msg.serialize();
        let mut guard = self.peers.lock().await;
        for socket in guard.iter_mut() {
            if let Err(e) = socket.lock().await.write_all(&serialized).await {
                eprintln!("Failed to send inv to peer: {}", e);
            }
        }
        Ok(())
    }

    /// Establish an outbound connection to a peer and send a handshake
    /// message describing this node's view of the chain.
    ///
    /// This is a minimal implementation intended for mainnet nodes to
    /// begin forming a Rust-native P2P mesh; full peer management and
    /// message handling will be layered on top of this.
    pub async fn connect_and_handshake(
        &self,
        addr: SocketAddr,
        local_height_val: u64,
        agent: &str,
    ) -> Result<(), String> {
        if self.is_connected(&addr).await {
            return Ok(());
        }
        if self.is_self_ip(addr.ip()).await {
            return Ok(());
        }
        if self.is_ip_connected(addr.ip()).await {
            return Ok(());
        }
        if self.is_banned(&addr.ip()) {
            return Err(format!("peer {} is banned (banlist)", addr));
        }
        // Simple jittered delay before connecting to avoid thundering herd.
        let jitter_ms = (rand_core::OsRng.next_u32() % 5000) as u64;
        tokio::time::sleep(std::time::Duration::from_millis(jitter_ms)).await;
        let mut stream = TcpStream::connect(addr)
            .await
            .map_err(|e| format!("connect to {} failed: {}", addr, e))?;

        // Check reputation before keeping a long-lived connection.
        {
            let peer_id = addr.to_string();
            let mut rep = self.reputation.lock().await;
            if rep.is_banned(&peer_id) {
                return Err(format!("peer {} is banned", peer_id));
            }
            rep.record_success(&peer_id);
        }

        Self::log(format!(
            "P2P outbound {}: connected, awaiting challenge",
            addr
        ));
        // Expect a sybil challenge from the remote and respond with a proof
        // before proceeding with the normal handshake.
        let challenge_msg = match read_message_with_timeout(&mut stream, Self::peer_timeout()).await {
            Ok(m) => m,
            Err(e) => {
                let mut rep = self.reputation.lock().await;
                rep.record_failure(&addr.to_string());
                return Err(e);
            }
        };
        if challenge_msg.msg_type != MessageType::SybilChallenge {
            let mut rep = self.reputation.lock().await;
            rep.record_failure(&addr.to_string());
            return Err("expected sybil challenge from peer".to_string());
        }
        let challenge = match SybilChallenge::from_bytes(&challenge_msg.payload) {
            Some(c) => c,
            None => {
                let mut rep = self.reputation.lock().await;
                rep.record_failure(&addr.to_string());
                return Err("invalid sybil challenge payload".to_string());
            }
        };

        // Bind proof-of-work to a persistent node identity derived from disk.
        let peer_pubkey = self.node_id.clone();
        let handshake = SybilResistantHandshake::new(challenge.difficulty);
        let proof = handshake
            .solve_challenge(challenge, peer_pubkey.to_vec())
            .map_err(|e| format!("failed to solve sybil challenge: {}", e))?;
        let proof_bytes = proof.to_bytes();
        let proof_msg = Message {
            msg_type: MessageType::SybilProof,
            payload: proof_bytes,
        };
        let proof_ser = proof_msg.serialize();
        stream
            .write_all(&proof_ser)
            .await
            .map_err(|e| format!("send sybil proof to {} failed: {}", addr, e))?;
        Self::log(format!("P2P outbound {}: sent sybil proof", addr));

        let payload = HandshakePayload {
            version: 1,
            agent: agent.to_string(),
            height: local_height_val,
            timestamp: Utc::now().timestamp(),
            port: self.bind_addr.port(),
            checkpoint_height: None,
            checkpoint_hash: None,
            relay_address: self.relay_address.clone(),
            node_id: Some(hex::encode(&self.node_id)),
        };

        let msg = payload
            .to_message()
            .map_err(|e| format!("build handshake message failed: {}", e))?;
        let bytes = msg.serialize();

        stream
            .write_all(&bytes)
            .await
            .map_err(|e| format!("send handshake to {} failed: {}", addr, e))?;
        Self::log(format!("P2P outbound {}: sent handshake", addr));

        let (mut reader, writer_half) = stream.into_split();
        let writer = Arc::new(tokio::sync::Mutex::new(writer_half));

        {
            let mut guard = self.peers.lock().await;
            guard.push(writer.clone());
        }
        {
            let mut guard = self.connected.lock().await;
            guard.insert(addr);
        }

        {
            let mut dir = self.peers_directory.lock().await;
            let multiaddr = format!("/ip4/{}/tcp/{}", addr.ip(), addr.port());
            dir.register_connection(multiaddr, None, self.relay_address.clone(), None);
        }

        let ping_writer = writer.clone();
        let ping_addr = addr;
        let ping_chain = self.chain.clone();
        let ping_agent = agent.to_string();
        let ping_relay = self.relay_address.clone();
        let ping_port = self.bind_addr.port();
        let ping_node_id = self.node_id.clone();
        tokio::spawn(async move {
            let interval = P2PNode::ping_interval();
            let mut last_height = crate::p2p::local_height(&ping_chain);
            let mut last_handshake = Instant::now();
            loop {
                tokio::time::sleep(interval).await;
                let nonce = rand_core::OsRng.next_u64();
                let ping = PingPayload { nonce };
                let msg = ping.to_message();
                if let Err(e) = send_message(&ping_writer, msg, ping_addr).await {
                    P2PNode::log_event(
                        "warn",
                        "net",
                        format!("P2P {}: ping failed: {}", ping_addr, e),
                    );
                    break;
                }
                let current_height = crate::p2p::local_height(&ping_chain);
                let handshake_due = last_handshake.elapsed() >= P2PNode::handshake_interval();
                if handshake_due || current_height != last_height {
                    let payload = HandshakePayload {
                        version: 1,
                        agent: ping_agent.clone(),
                        height: current_height,
                        timestamp: Utc::now().timestamp(),
                        port: ping_port,
                        checkpoint_height: None,
                        checkpoint_hash: None,
                        relay_address: ping_relay.clone(),
                        node_id: Some(hex::encode(&ping_node_id)),
                    };
                    if let Ok(msg) = payload.to_message() {
                        let _ = send_message(&ping_writer, msg, ping_addr).await;
                    }
                    last_height = current_height;
                    last_handshake = Instant::now();
                }
            }
        });

        let dir = self.peers_directory.clone();
        let relay_addr = self.relay_address.clone();
        let chain_for_sync = self.chain.clone();
        let mempool_for_sync = self.mempool.clone();
        let reputation = self.reputation.clone();
        let sync_requests = self.sync_requests.clone();
        let block_requests = self.block_requests.clone();
        let getblocks_seen = self.getblocks_seen.clone();
        let getblocks_last = self.getblocks_last.clone();
        let self_ips = self.self_ips.clone();
        let peers_vec = self.peers.clone();
        let connected_vec = self.connected.clone();
        let writer_for_drop = writer.clone();
        let local_node_id = hex::encode(&self.node_id);
        tokio::spawn(async move {
            let mut msg_count: u32 = 0;
            let mut window_start = Instant::now();
            let mut last_handshake_height: Option<u64> = None;
            let mut last_handshake_agent: Option<String> = None;
            loop {
                if window_start.elapsed() < Duration::from_secs(1) {
                    msg_count += 1;
                    if msg_count > MAX_MSGS_PER_SEC {
                        Self::log_err(format!("P2P outbound {}: rate limit", addr));
                        break;
                    }
                } else {
                    window_start = Instant::now();
                    msg_count = 1;
                }
                match read_message_with_timeout(&mut reader, P2PNode::peer_timeout()).await {
                    Ok(msg) => {
                        if P2PNode::verbose_messages() {
                            match msg.msg_type {
                                MessageType::Ping | MessageType::Pong | MessageType::Handshake | MessageType::Peers | MessageType::GetPeers | MessageType::GetHeaders | MessageType::GetBlocks | MessageType::Headers => {}
                                _ => {
                                    P2PNode::log_event(
                                        "info",
                                        "net",
                                        format!("P2P {}: recv {:?}", addr, msg.msg_type),
                                    );
                                }
                            }
                        }
                        match msg.msg_type {
                            MessageType::Ping => {
                                if let Ok(ping) = PingPayload::from_message(&msg) {
                                    let mut payload = Vec::new();
                                    payload.extend_from_slice(&ping.nonce.to_be_bytes());
                                    let pong = Message {
                                        msg_type: MessageType::Pong,
                                        payload,
                                    };
                                    let _ = send_message(&writer, pong, addr).await;
                                    let mut dir_guard = dir.lock().await;
                                    let multiaddr = format!("/ip4/{}/tcp/{}", addr.ip(), addr.port());
                                    dir_guard.mark_seen(&multiaddr);
                                }
                            }
                            MessageType::Handshake => {
                                if let Ok(payload) = HandshakePayload::from_message(&msg) {
                                    if let Some(ref remote_id) = payload.node_id {
                                        if remote_id == &local_node_id {
                                            {
                                                let mut guard = self_ips.lock().await;
                                                guard.insert(addr.ip());
                                            }
                                            P2PNode::log_event(
                                                "warn",
                                                "net",
                                                format!("P2P {}: self-connection detected, closing", addr),
                                            );
                                            break;
                                        }
                                    }
                                    let agent_str = payload.agent.clone();
                                    let node_id = payload.node_id.clone();
                                    {
                                        let mut dir_guard = dir.lock().await;
                                        let multiaddr =
                                            format!("/ip4/{}/tcp/{}", addr.ip(), addr.port());
                                        dir_guard.register_connection(
                                            multiaddr.clone(),
                                            Some(agent_str.clone()),
                                            payload.relay_address.clone(),
                                            node_id.clone(),
                                        );
                                        dir_guard.record_height(&multiaddr, payload.height);
                                    }
                                    let should_log = last_handshake_height != Some(payload.height)
                                        || last_handshake_agent.as_deref() != Some(agent_str.as_str());
                                    if should_log {
                                        Self::log(format!(
                                            "P2P outbound {}: received handshake (agent {}, height {})",
                                            addr, agent_str, payload.height
                                        ));
                                    }
                                    last_handshake_height = Some(payload.height);
                                    last_handshake_agent = Some(agent_str.clone());
                                    // If we have a relay address, advertise it back.
                                    if let Some(relay) = relay_addr.clone() {
                                        let relay_msg = RelayAddressPayload {
                                            txid: String::new(),
                                            address: relay,
                                        };
                                        if let Ok(msg) = relay_msg.to_message() {
                                            let _ = send_message(&writer, msg, addr).await;
                                        }
                                    }
                                    // Ask for peers to grow the mesh.
                                    if let Ok(msg) = EmptyPayload::to_message(MessageType::GetPeers)
                                    {
                                        let _ = send_message(&writer, msg, addr).await;
                                    }
                                    // Basic header-first sync trigger: if peer is ahead, request blocks.
                                    let local_height = {
                                        if let Some(ref c) = chain_for_sync {
                                            c.lock().unwrap().tip_height()
                                        } else {
                                            0
                                        }
                                    };
                                    if payload.height > local_height {
                                        if sync_request_allowed(&sync_requests, addr.ip()).await {
                                            let start_hash = P2PNode::tip_hash(&chain_for_sync);
                                            let get_headers = GetHeadersPayload {
                                                start_hash: start_hash.to_vec(),
                                                count: 64,
                                            };
                                            let short = {
                                                let h = hex::encode(start_hash);
                                                h.get(0..12).unwrap_or(&h).to_string()
                                            };
                                            Self::log(format!(
                                                "P2P {}: peer ahead ({} > {}), requesting headers from tip {}",
                                                addr,
                                                payload.height,
                                                local_height,
                                                short
                                            ));
                                            if let Ok(msg) = get_headers.to_message() {
                                                let _ = send_message(&writer, msg, addr).await;
                                            }
                                        }
                                    } else if payload.height < local_height {
                                        // Peer is behind; push headers and fall back to block push if needed.
                                        let behind_height = payload.height;
                                        if let Some(ref chain_arc) = chain_for_sync {
                                            let headers_bytes = {
                                                let guard = chain_arc.lock().unwrap();
                                                let start = payload.height.saturating_add(1) as usize;
                                                let mut headers = Vec::new();
                                                for block in guard.chain.iter().skip(start).take(32) {
                                                    headers.extend_from_slice(&block.header.serialize());
                                                }
                                                headers
                                            };
                                            if !headers_bytes.is_empty() {
                                                let msg = HeadersPayload { headers: headers_bytes }.to_message();
                                                let _ = send_message(&writer, msg, addr).await;
                                            }
                                        }
                                        let fallback_writer = writer.clone();
                                        let fallback_chain = chain_for_sync.clone();
                                        let fallback_seen = getblocks_seen.clone();
                                        let fallback_addr = addr;
                                        tokio::spawn(async move {
                                            let grace = getblocks_grace();
                                            tokio::time::sleep(grace).await;
                                            let now = Instant::now();
                                            {
                                                let mut guard = fallback_seen.lock().await;
                                                if let Some(last) = guard.get(&fallback_addr.ip()) {
                                                    if now.duration_since(*last) < grace {
                                                        return;
                                                    }
                                                }
                                                guard.insert(fallback_addr.ip(), now);
                                            }
                                            let (blocks, start_height) = if let Some(ref chain_arc) = fallback_chain {
                                                let guard = chain_arc.lock().unwrap();
                                                let start_idx = behind_height.saturating_add(1) as usize;
                                                if start_idx >= guard.chain.len() {
                                                    (Vec::new(), 0)
                                                } else {
                                                    let mut blocks = Vec::new();
                                                    for b in guard.chain.iter().skip(start_idx).take(32) {
                                                        blocks.push(b.serialize());
                                                    }
                                                    (blocks, start_idx as u64)
                                                }
                                            } else {
                                                (Vec::new(), 0)
                                            };
                                            if blocks.is_empty() {
                                                return;
                                            }
                                            let end_h = start_height + blocks.len() as u64 - 1;
                                            P2PNode::log_event(
                                                "info",
                                                "sync",
                                                format!(
                                                    "P2P {}: no getblocks after headers, pushing {} blocks [{}-{}]",
                                                    fallback_addr,
                                                    blocks.len(),
                                                    start_height,
                                                    end_h
                                                ),
                                            );
                                            for block_data in blocks {
                                                let msg = BlockPayload { block_data }.to_message();
                                                let _ = send_message(&fallback_writer, msg, fallback_addr).await;
                                            }
                                        });
                                    }
                                }
                            }
                            MessageType::Pong => {
                                let mut dir_guard = dir.lock().await;
                                let multiaddr = format!("/ip4/{}/tcp/{}", addr.ip(), addr.port());
                                dir_guard.mark_seen(&multiaddr);
                                                            }
                            MessageType::GetPeers => {
                                let peers_payload = {
                                    let dir = dir.lock().await;
                                    PeersPayload {
                                        peers: dir.peers().iter().map(|p| p.multiaddr.clone()).collect(),
                                    }
                                };
                                if let Ok(resp) = peers_payload.to_message() {
                                    let _ = send_message(&writer, resp, addr).await;
                                }
                            }
                            MessageType::Peers => {
                                if let Ok(list) = PeersPayload::from_message(&msg) {
                                    let mut dir = dir.lock().await;
                                    for p in list.peers {
                                        dir.register_peer_hint(p);
                                    }
                                }
                            }
                            MessageType::GetHeaders => {
                                if let Some(ref chain_arc) = chain_for_sync {
                                    if let Ok(payload) = GetHeadersPayload::from_message(&msg) {
                                        let headers_bytes = {
                                            let guard = chain_arc.lock().unwrap();

                                            let mut start_idx = 0usize;
                                            if !payload.start_hash.is_empty() && payload.start_hash.len() == 32 {
                                                let mut target = [0u8; 32];
                                                target.copy_from_slice(&payload.start_hash);
                                                if let Some(pos) =
                                                    guard.chain.iter().position(|b| b.header.hash() == target)
                                                {
                                                    start_idx = pos;
                                                }
                                            }

                                            let count = payload.count.min(MAX_HEADERS_PER_REQUEST) as usize;
                                            let mut bytes = Vec::new();
                                            for block in guard
                                                .chain
                                                .iter()
                                                .skip(start_idx)
                                                .take(count)
                                            {
                                                bytes.extend_from_slice(&block.header.serialize());
                                            }
                                            bytes
                                        };

                                        let msg = HeadersPayload { headers: headers_bytes }.to_message();
                                        let _ = send_message(&writer, msg, addr).await;
                                    }
                                }
                            }
                            MessageType::Headers => {
                                if let Some(ref chain_arc) = chain_for_sync {
                                    if let Ok(payload) = HeadersPayload::from_message(&msg) {
                                        let mut offset = 0usize;
                                        while offset + 80 <= payload.headers.len() {
                                            let slice = &payload.headers[offset..offset + 80];
                                            let (header, used) = match crate::block::BlockHeader::deserialize(slice)
                                            {
                                                Ok(v) => v,
                                                Err(e) => {
                                                    eprintln!("Failed to parse header from {}: {}", addr, e);
                                                    break;
                                                }
                                            };
                                            offset += used;

                                            {
                                                let mut guard = chain_arc.lock().unwrap();
                                                if let Err(e) = guard.add_header(header.clone()) {
                                                    eprintln!("Header from {} rejected: {}", addr, e);
                                                    break;
                                                }
                                            }
                                        }

                                        if let Some(ref chain_arc) = chain_for_sync {
                                            let header_count = (payload.headers.len() / 80) as u32;
                                            let request = {
                                                let guard = chain_arc.lock().unwrap();
                                                if let Some(best) = guard.best_header_if_better() {
                                                    if let Some(path) =
                                                        guard.header_path_to_known(best.header.hash())
                                                    {
                                                        if let Some(first_hash) = path.first() {
                                                            let start_hash = guard
                                                                .headers
                                                                .get(first_hash)
                                                                .map(|hw| hw.header.prev_hash)
                                                                .unwrap_or([0u8; 32]);
                                                            let count = path.len() as u32;
                                                            if count > 0 {
                                                                Some((start_hash, count))
                                                            } else {
                                                                None
                                                            }
                                                        } else {
                                                            None
                                                        }
                                                    } else {
                                                        None
                                                    }
                                                } else {
                                                    None
                                                }
                                            };
                                            let fallback = if request.is_none() && header_count > 0 {
                                                Some((P2PNode::tip_hash(&chain_for_sync), header_count))
                                            } else {
                                                None
                                            };
                                            if let Some((start_hash, count)) = request.or(fallback) {
                                                let get_blocks = GetBlocksPayload {
                                                    start_hash: start_hash.to_vec(),
                                                    count,
                                                };
                                                if sync_block_request_allowed(&block_requests, addr.ip()).await {
                                                    if let Ok(msg) = get_blocks.to_message() {
                                                        let _ = send_message(&writer, msg, addr).await;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            MessageType::GetBlocks => {
                                {
                                    let mut guard = getblocks_seen.lock().await;
                                    guard.insert(addr.ip(), Instant::now());
                                }
                                if let Some(ref chain_arc) = chain_for_sync {
                                    if let Ok(payload) = GetBlocksPayload::from_message(&msg) {
                                        if !getblocks_request_allowed(
                                            &getblocks_last,
                                            addr.ip(),
                                            &payload.start_hash,
                                            payload.count,
                                        ).await
                                        {
                                            continue;
                                        }
                                        let (blocks, start_height) = {
                                            let guard = chain_arc.lock().unwrap();
                                            let mut start_idx = 0usize;
                                            if payload.start_hash.len() == 32 {
                                                let mut target = [0u8; 32];
                                                target.copy_from_slice(&payload.start_hash);
                                                if let Some(pos) =
                                                    guard.chain.iter().position(|b| b.header.hash() == target)
                                                {
                                                    start_idx = pos + 1;
                                                }
                                            }
                                            let mut blocks = Vec::new();
                                            let mut heights = Vec::new();
                                            let count = payload.count.min(MAX_BLOCKS_PER_REQUEST) as usize;
                                            for (idx, b) in guard
                                                .chain
                                                .iter()
                                                .enumerate()
                                                .skip(start_idx)
                                                .take(count)
                                            {
                                                blocks.push(b.serialize());
                                                heights.push(idx as u64);
                                            }
                                            let start_h: u64 = heights.first().copied().unwrap_or(0);
                                            (blocks, start_h)
                                        };
                                        if !blocks.is_empty() {
                                            let end_h = start_height + blocks.len() as u64 - 1;
                                            P2PNode::log(format!(
                                                "P2P {}: sending {} blocks [{}-{}]",
                                                addr,
                                                blocks.len(),
                                                start_height,
                                                end_h
                                            ));
                                        }
                                        for block_data in blocks {
                                            let msg = BlockPayload { block_data }.to_message();
                                            let _ = send_message(&writer, msg, addr).await;
                                        }
                                    }
                                }
                            }
                            MessageType::Block => {
                                if let Some(ref chain_arc) = chain_for_sync {
                                    if let Ok(payload) = BlockPayload::from_message(&msg) {
                                        match Block::deserialize(&payload.block_data) {
                                            Ok((block, _)) => {
                                                let bhash = block.header.hash();
                                                let short = hex::encode(bhash);
                                                let short = short.get(0..12).unwrap_or(&short);
                                                let (new_height_opt, record_verdict, persist_height) = {
                                                    let mut guard = chain_arc.lock().unwrap();
                                                    let mut new_height_opt = None;
                                                    let mut record_verdict = None;
                                                    let mut persist_height = None;
                                                    match guard.process_block(block.clone()) {
                                                        Ok((new_height, _tip)) => {
                                                            P2PNode::log(format!(
                                                                "P2P {}: accepted block height {} hash {}",
                                                                addr, new_height.saturating_sub(1), short
                                                            ));
                                                            if let Some(ref mem) = mempool_for_sync {
                                                                let mut mem_guard = mem.lock().unwrap();
                                                                for tx in block.transactions.iter().skip(1) {
                                                                    mem_guard.remove(&tx.txid());
                                                                }
                                                            }
                                                            guard.headers.clear();
                                                            guard.header_chain.clear();
                                                            new_height_opt = Some(new_height);
                                                            record_verdict = Some(true);
                                                            if guard.tip_hash() == bhash {
                                                                persist_height = Some(guard.tip_height());
                                                            }
                                                        }
                                                        Err(e) => {
                                                            if P2PNode::is_soft_block_reject(&e) {
                                                                if !P2PNode::is_duplicate_block(&e) {
                                                                    P2PNode::log_event(
                                                                        "info",
                                                                        "chain",
                                                                        format!(
                                                                            "P2P {}: ignored block {} ({})",
                                                                            addr, short, e
                                                                        ),
                                                                    );
                                                                }
                                                            } else {
                                                                P2PNode::log_event(
                                                                    "warn",
                                                                    "chain",
                                                                    format!(
                                                                        "P2P {}: rejected block {}: {}",
                                                                        addr, short, e
                                                                    ),
                                                                );
                                                                record_verdict = Some(false);
                                                            }
                                                        }
                                                    }
                                                    (new_height_opt, record_verdict, persist_height)
                                                };
                                                if let Some(height) = persist_height {
                                                    if let Err(e) = storage::write_block_json(height, &block) {
                                                        P2PNode::log_event(
                                                            "warn",
                                                            "chain",
                                                            format!(
                                                                "P2P {}: failed to persist block {}: {}",
                                                                addr, short, e
                                                            ),
                                                        );
                                                    }
                                                }
                                                if let Some(new_height) = new_height_opt {
                                                    let multiaddr = format!(
                                                        "/ip4/{}/tcp/{}",
                                                        addr.ip(),
                                                        addr.port()
                                                    );
                                                    let mut directory = dir.lock().await;
                                                    directory.record_height(
                                                        &multiaddr,
                                                        new_height.saturating_sub(1),
                                                    );
                                                }
                                                if let Some(ok) = record_verdict {
                                                    let mut rep = reputation.lock().await;
                                                    rep.record_block(&addr.to_string(), ok);
                                                }
                                            }
                                            Err(e) => {
                                                eprintln!("Failed to decode block payload from {}: {}", addr, e);
                                                let mut rep = reputation.lock().await;
                                                rep.record_decode_error(&addr.to_string());
                                            }
                                        }
                                    }
                                }
                            }
                            MessageType::Tx => {
                                if let Ok(payload) = TxPayload::from_message(&msg) {
                                    match decode_full_tx(&payload.tx_data) {
                                        Ok(tx) => {
                                            if let (Some(ref chain_arc), Some(ref mem)) =
                                                (&chain_for_sync, &mempool_for_sync)
                                            {
                                                let inv_bytes = {
                                                    let fee = {
                                                        let guard = chain_arc.lock().unwrap();
                                                        match guard.calculate_fees(&tx) {
                                                            Ok(f) => f,
                                                            Err(e) => {
                                                                eprintln!("Rejecting tx from {}: {}", addr, e);
                                                                continue;
                                                            }
                                                        }
                                                    };
                                                    let relay_addr = {
                                                        let dir = dir.lock().await;
                                                        dir.relay_address_for_peer(&addr)
                                                    };
                                                    let mut mem_guard = mem.lock().unwrap();
                                                    let peer_addr = addr.to_string();
                                                    let txid = tx.txid();
                                                    let txid_hex = hex::encode(txid);
                                                    match mem_guard.add_transaction(
                                                        tx.clone(),
                                                        payload.tx_data.clone(),
                                                        fee,
                                                    ) {
                                                        Ok(outcome) => {
                                                            P2PNode::log_event(
                                                                "info",
                                                                "mempool",
                                                                format!(
                                                                    "P2P {}: accepted tx {} fee {}",
                                                                    addr, txid_hex, fee
                                                                ),
                                                            );
                                                            if let Some(evicted) = outcome.evicted {
                                                                let evicted_hex = hex::encode(evicted);
                                                                P2PNode::log_event(
                                                                    "info",
                                                                    "mempool",
                                                                    format!(
                                                                        "P2P {}: evicted tx {}",
                                                                        addr, evicted_hex
                                                                    ),
                                                                );
                                                            }
                                                            mem_guard.record_relay(&outcome.txid, peer_addr.clone());
                                                        }
                                                        Err(e) => {
                                                            P2PNode::log_event(
                                                                "warn",
                                                                "mempool",
                                                                format!(
                                                                    "P2P {}: rejected tx {}: {}",
                                                                    addr, txid_hex, e
                                                                ),
                                                            );
                                                            mem_guard.record_relay(&txid, peer_addr);
                                                        }
                                                    }
                                                    if let Some(relay_addr) = relay_addr {
                                                        mem_guard.record_relay_address(&txid, relay_addr);
                                                    }
                                                    InvPayload {
                                                        txids: vec![hex::encode(tx.txid())],
                                                    }
                                                    .to_message()
                                                    .ok()
                                                    .map(|m| m.serialize())
                                                };

                                                if let Some(inv_bytes) = inv_bytes {
                                                    let mut peers_guard = peers_vec.lock().await;
                                                    for socket in peers_guard.iter_mut() {
                                                        let _ = socket.lock().await.write_all(&inv_bytes).await;
                                                    }
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("Failed to decode tx from {}: {}", addr, e);
                                        }
                                    }
                                }
                            }
                            MessageType::Inv => {
                                if let Some(ref mem) = mempool_for_sync {
                                    if let Ok(inv) = InvPayload::from_message(&msg) {
                                        let mut needed = Vec::new();
                                        {
                                            let guard = mem.lock().unwrap();
                                            for txid_hex in inv.txids {
                                                if let Ok(bytes) = hex::decode(&txid_hex) {
                                                    if bytes.len() == 32 {
                                                        let mut txid = [0u8; 32];
                                                        txid.copy_from_slice(&bytes);
                                                        if !guard.contains(&txid) {
                                                            needed.push(txid_hex.clone());
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        if !needed.is_empty() {
                                            let gd = GetDataPayload { txids: needed };
                                            if let Ok(msg) = gd.to_message() {
                                                let _ = send_message(&writer, msg, addr).await;
                                            }
                                        }
                                    }
                                }
                            }
                            MessageType::GetData => {
                                if let Some(ref mem) = mempool_for_sync {
                                    if let Ok(gd) = GetDataPayload::from_message(&msg) {
                                        let mut responses: Vec<Message> = Vec::new();
                                        {
                                            let guard = mem.lock().unwrap();
                                            for txid_hex in gd.txids {
                                                if let Ok(bytes) = hex::decode(&txid_hex) {
                                                    if bytes.len() != 32 {
                                                        continue;
                                                    }
                                                    let mut txid = [0u8; 32];
                                                    txid.copy_from_slice(&bytes);
                                                    if let Some(raw) = guard.raw_tx(&txid) {
                                                        responses.push(TxPayload { tx_data: raw }.to_message());
                                                    }
                                                }
                                            }
                                        }
                                        for msg in responses {
                                            let _ = send_message(&writer, msg, addr).await;
                                        }
                                    }
                                }
                            }
                            MessageType::Mempool => {
                                if let Some(ref mem) = mempool_for_sync {
                                    let tx_hashes: Vec<String> = {
                                        let guard = mem.lock().unwrap();
                                        guard.txids_hex()
                                    };
                                    let payload = MempoolPayload { tx_hashes };
                                    if let Ok(msg) = payload.to_message() {
                                        let _ = send_message(&writer, msg, addr).await;
                                    }
                                } else if let Ok(msg) = EmptyPayload::to_message(MessageType::Mempool) {
                                    let _ = send_message(&writer, msg, addr).await;
                                }
                            }
                            MessageType::RelayAddress => {
                                if let Some(ref mem) = mempool_for_sync {
                                    if let Ok(relay) = RelayAddressPayload::from_message(&msg) {
                                        if relay.txid.len() == 64 {
                                            if let Ok(bytes) = hex::decode(relay.txid) {
                                                if bytes.len() == 32 {
                                                    let mut txid = [0u8; 32];
                                                    txid.copy_from_slice(&bytes);
                                                    let mut guard = mem.lock().unwrap();
                                                    guard.record_relay_address(&txid, relay.address);
                                                }
                                            }
                                        } else if relay.txid.is_empty() {
                                            let multiaddr = format!("/ip4/{}/tcp/{}", addr.ip(), addr.port());
                                            let mut dir = dir.lock().await;
                                            dir.register_connection(multiaddr, None, Some(relay.address), None);
                                        }
                                    }
                                }
                            }
                            MessageType::Disconnect => break,
                            _ => {}
                        }
                    }
                    Err(e) => {
                        Self::log_err(format!("P2P outbound {}: closing read loop: {}", addr, e));
                        let mut rep = reputation.lock().await;
                        rep.record_failure(&addr.to_string());
                        break;
                    }
                }
            }
            {
                let mut guard = peers_vec.lock().await;
                guard.retain(|p| !Arc::ptr_eq(p, &writer_for_drop));
            }
            {
                let mut guard = connected_vec.lock().await;
                guard.remove(&addr);
            }
        });

        Ok(())
    }
}

/// Read a single protocol message from the given TCP stream.
async fn read_message<R>(stream: &mut R) -> Result<Message, String>
where
    R: AsyncReadExt + Unpin,
{
    let mut header = [0u8; 6];
    stream
        .read_exact(&mut header)
        .await
        .map_err(|e| format!("failed to read message header: {}", e))?;

    let version = header[0];
    if version != crate::protocol::PROTOCOL_VERSION {
        return Err(format!("unsupported protocol version: {}", version));
    }

    let _msg_type = header[1];
    let mut len_bytes = [0u8; 4];
    len_bytes.copy_from_slice(&header[2..6]);
    let length = u32::from_be_bytes(len_bytes);
    if length > MAX_MESSAGE_SIZE {
        return Err("message too large".to_string());
    }

    let mut payload = vec![0u8; length as usize];
    if length > 0 {
        stream
            .read_exact(&mut payload)
            .await
            .map_err(|e| format!("failed to read message payload: {}", e))?;
    }

    Message::deserialize(&[&header[..], &payload[..]].concat())
}

async fn read_message_with_timeout<R>(
    stream: &mut R,
    timeout: Duration,
) -> Result<Message, String>
where
    R: AsyncReadExt + Unpin,
{
    match tokio::time::timeout(timeout, read_message(stream)).await {
        Ok(res) => res,
        Err(_) => Err("peer read timeout".to_string()),
    }
}

async fn send_message(
    writer: &Arc<Mutex<OwnedWriteHalf>>,
    msg: Message,
    peer: SocketAddr,
) -> Result<(), String> {
    let bytes = msg.serialize();
    writer
        .lock()
        .await
        .write_all(&bytes)
        .await
        .map_err(|e| format!("failed to send to {}: {}", peer, e))
}

fn local_height(chain: &Option<Arc<StdMutex<ChainState>>>) -> u64 {
    chain
        .as_ref()
        .and_then(|c| c.lock().ok().map(|g| g.tip_height()))
        .unwrap_or(0)
}

/// Handle an incoming peer connection by performing sybil-resistant
/// handshake verification before accepting the peer into the set of
/// connected sockets.
async fn handle_incoming_with_sybil(
    mut socket: TcpStream,
    addr: SocketAddr,
    bind_addr: SocketAddr,
    peers: Arc<Mutex<Vec<Arc<Mutex<OwnedWriteHalf>>>>>,
    connected: Arc<Mutex<HashSet<SocketAddr>>>,
    directory: Arc<Mutex<PeerDirectory>>,
    reputation: Arc<Mutex<ReputationManager>>,
    sync_requests: Arc<Mutex<HashMap<IpAddr, Instant>>>,
    block_requests: Arc<Mutex<HashMap<IpAddr, Instant>>>,
    getblocks_seen: Arc<Mutex<HashMap<IpAddr, Instant>>>,
    getblocks_last: Arc<Mutex<HashMap<IpAddr, (Vec<u8>, u32, Instant)>>>,
    self_ips: Arc<Mutex<HashSet<IpAddr>>>,
    chain: Option<Arc<StdMutex<ChainState>>>,
    mempool: Option<Arc<StdMutex<MempoolManager>>>,
    agent: String,
    relay_address: Option<String>,
    node_id: Vec<u8>,
) -> Result<(), String> {
    let local_node_id = hex::encode(&node_id);
    // Issue a fresh challenge with adaptive difficulty.
    let base = P2PNode::sybil_difficulty();
    let max = std::env::var("IRIUM_SYBIL_DIFFICULTY_MAX")
        .ok()
        .and_then(|v| v.parse::<u8>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(20);
    let banned = {
        let rep = reputation.lock().await;
        rep.banned_count() as u8
    };
    let difficulty = std::cmp::min(max, base.saturating_add(banned.min(5)));
    let handshake = SybilResistantHandshake::new(difficulty);
    let challenge = handshake.create_challenge();
    let challenge_bytes = challenge.to_bytes();
    let challenge_msg = Message {
        msg_type: MessageType::SybilChallenge,
        payload: challenge_bytes,
    };
    let ser = challenge_msg.serialize();
    socket
        .write_all(&ser)
        .await
        .map_err(|e| format!("failed to send sybil challenge to {}: {}", addr, e))?;

    // Expect a proof in response.
    let proof_msg = match read_message_with_timeout(&mut socket, P2PNode::peer_timeout()).await {
        Ok(m) => m,
        Err(e) => {
            if e.contains("early eof") {
                let peer_id = addr.to_string();
                let mut rep = reputation.lock().await;
                for _ in 0..5 {
                    rep.record_failure(&peer_id);
                }
                return Err(format!("early eof during sybil proof from {}: {}", addr, e));
            }
            return Err(e);
        }
    };
    if proof_msg.msg_type != MessageType::SybilProof {
        return Err("expected sybil proof from peer".to_string());
    }
    let proof = SybilProof::from_bytes(&proof_msg.payload)
        .ok_or_else(|| "invalid sybil proof payload".to_string())?;
    if !handshake.verify_proof(&proof) {
        {
            let mut rep = reputation.lock().await;
            rep.record_failure(&addr.to_string());
        }
        return Err("sybil proof verification failed".to_string());
    }

    {
        let mut rep = reputation.lock().await;
        rep.record_success(&addr.to_string());
    }
    {
        let mut guard = connected.lock().await;
        guard.insert(addr);
    }
    // At this point, accept the peer and start reading further messages.
    let (mut reader, writer_half) = socket.into_split();
    let writer = Arc::new(tokio::sync::Mutex::new(writer_half));
    {
        let mut guard = peers.lock().await;
        guard.push(writer.clone());
    }

    let ping_writer = writer.clone();
    let ping_addr = addr;
    let ping_chain = chain.clone();
    let ping_agent = agent.clone();
    let ping_relay = relay_address.clone();
    let ping_port = bind_addr.port();
    let ping_node_id = node_id.clone();
    tokio::spawn(async move {
        let interval = P2PNode::ping_interval();
        let mut last_height = crate::p2p::local_height(&ping_chain);
        let mut last_handshake = Instant::now();
        loop {
            tokio::time::sleep(interval).await;
            let nonce = rand_core::OsRng.next_u64();
            let ping = PingPayload { nonce };
            let msg = ping.to_message();
            if let Err(e) = send_message(&ping_writer, msg, ping_addr).await {
                P2PNode::log_event(
                    "warn",
                    "net",
                    format!("P2P {}: ping failed: {}", ping_addr, e),
                );
                break;
            }
            let current_height = crate::p2p::local_height(&ping_chain);
            let handshake_due = last_handshake.elapsed() >= P2PNode::handshake_interval();
            if handshake_due || current_height != last_height {
                let payload = HandshakePayload {
                    version: 1,
                    agent: ping_agent.clone(),
                    height: current_height,
                    timestamp: Utc::now().timestamp(),
                    port: ping_port,
                    checkpoint_height: None,
                    checkpoint_hash: None,
                    relay_address: ping_relay.clone(),
                    node_id: Some(hex::encode(&ping_node_id)),
                };
                if let Ok(msg) = payload.to_message() {
                    let _ = send_message(&ping_writer, msg, ping_addr).await;
                }
                last_height = current_height;
                last_handshake = Instant::now();
            }
        }
    });

    // Register the peer in the directory for future runtime seedlist updates.
    {
        let mut dir = directory.lock().await;
        let multiaddr = format!("/ip4/{}/tcp/{}", addr.ip(), addr.port());
        let node_id = hex::encode(&proof.peer_pubkey);
        dir.register_connection(multiaddr.clone(), None, None, Some(node_id));
        dir.record_height(&multiaddr, local_height(&chain));
    }

    // Reply with our handshake so outbound peers learn our agent/height.
    let local_h = local_height(&chain);
    let payload = HandshakePayload {
        version: 1,
        agent: agent.clone(),
        height: local_h,
        timestamp: Utc::now().timestamp(),
        port: bind_addr.port(),
        checkpoint_height: None,
        checkpoint_hash: None,
        relay_address: relay_address.clone(),
        node_id: Some(hex::encode(&node_id)),
    };
    if let Ok(msg) = payload.to_message() {
        let _ = send_message(&writer, msg, addr).await;
    }

    let mut msg_count: u32 = 0;
    let mut window_start = Instant::now();
    // Process messages from the peer.
    loop {
        if window_start.elapsed() < Duration::from_secs(1) {
            msg_count += 1;
            if msg_count > MAX_MSGS_PER_SEC {
                return Err("message rate limit exceeded".to_string());
            }
        } else {
            window_start = Instant::now();
            msg_count = 1;
        }
        let msg = match read_message_with_timeout(&mut reader, P2PNode::peer_timeout()).await {
            Ok(m) => m,
            Err(e) => return Err(e),
        };

        if P2PNode::verbose_messages() {
            match msg.msg_type {
                MessageType::Ping | MessageType::Pong | MessageType::Handshake | MessageType::Peers | MessageType::GetPeers | MessageType::GetHeaders | MessageType::GetBlocks | MessageType::Headers => {}
                _ => {
                    P2PNode::log_event("info", "net", format!("P2P {}: recv {:?}", addr, msg.msg_type));
                }
            }
        }

        match msg.msg_type {
            MessageType::Handshake => {
                if let Ok(payload) = HandshakePayload::from_message(&msg) {
                    if let Some(ref remote_id) = payload.node_id {
                        if remote_id == &local_node_id {
                            {
                                let mut guard = self_ips.lock().await;
                                guard.insert(addr.ip());
                            }
                            P2PNode::log_event(
                                "warn",
                                "net",
                                format!("P2P {}: self-connection detected, dropping", addr),
                            );
                            break;
                        }
                    }
                    let advertised_port = if payload.port > 0 {
                        payload.port
                    } else {
                        bind_addr.port()
                    };
                    let multiaddr = format!("/ip4/{}/tcp/{}", addr.ip(), advertised_port);
                    {
                        let mut dir = directory.lock().await;
                        dir.register_connection(
                            multiaddr.clone(),
                            Some(payload.agent.clone()),
                            payload.relay_address.clone(),
                            payload.node_id.clone(),
                        );
                        dir.record_height(&multiaddr, payload.height);
                    }

                    let response = HandshakePayload {
                        version: payload.version,
                        agent: agent.clone(),
                        height: local_height(&chain),
                        timestamp: Utc::now().timestamp(),
                        port: bind_addr.port(),
                        checkpoint_height: None,
                        checkpoint_hash: None,
                        relay_address: relay_address.clone(),
                        node_id: Some(hex::encode(&node_id)),
                    };
                    if let Ok(handshake_msg) = response.to_message() {
                        let _ = send_message(&writer, handshake_msg, addr).await;
                    }
                    // Ask peer for its view of the network.
                    if let Ok(msg) = EmptyPayload::to_message(MessageType::GetPeers) {
                        let _ = send_message(&writer, msg, addr).await;
                    }
                    if payload.height > local_h {
                        // Request headers first for basic sync.
                        if sync_request_allowed(&sync_requests, addr.ip()).await {
                            let start_hash = P2PNode::tip_hash(&chain);
                            let get_headers = GetHeadersPayload {
                                start_hash: start_hash.to_vec(),
                                count: 64,
                            };
                            if let Ok(msg) = get_headers.to_message() {
                                let _ = send_message(&writer, msg, addr).await;
                            }
                        }
                    } else if payload.height < local_h {
                        // Peer is behind; send headers and fall back to block push if needed.
                        let behind_height = payload.height;
                        if let Some(ref chain_arc) = chain {
                            let headers_bytes = {
                                let guard = chain_arc.lock().unwrap();
                                let start = payload.height.saturating_add(1) as usize;
                                let mut headers = Vec::new();
                                for block in guard.chain.iter().skip(start).take(32) {
                                    headers.extend_from_slice(&block.header.serialize());
                                }
                                headers
                            };
                            if !headers_bytes.is_empty() {
                                let msg = HeadersPayload { headers: headers_bytes }.to_message();
                                let _ = send_message(&writer, msg, addr).await;
                            }
                        }
                        let fallback_writer = writer.clone();
                        let fallback_chain = chain.clone();
                        let fallback_seen = getblocks_seen.clone();
                        let fallback_addr = addr;
                        tokio::spawn(async move {
                            let grace = getblocks_grace();
                            tokio::time::sleep(grace).await;
                            let now = Instant::now();
                            {
                                let mut guard = fallback_seen.lock().await;
                                if let Some(last) = guard.get(&fallback_addr.ip()) {
                                    if now.duration_since(*last) < grace {
                                        return;
                                    }
                                }
                                guard.insert(fallback_addr.ip(), now);
                            }
                            let (blocks, start_height) = if let Some(ref chain_arc) = fallback_chain {
                                let guard = chain_arc.lock().unwrap();
                                let start_idx = behind_height.saturating_add(1) as usize;
                                if start_idx >= guard.chain.len() {
                                    (Vec::new(), 0)
                                } else {
                                    let mut blocks = Vec::new();
                                    for b in guard.chain.iter().skip(start_idx).take(32) {
                                        blocks.push(b.serialize());
                                    }
                                    (blocks, start_idx as u64)
                                }
                            } else {
                                (Vec::new(), 0)
                            };
                            if blocks.is_empty() {
                                return;
                            }
                            let end_h = start_height + blocks.len() as u64 - 1;
                            P2PNode::log_event(
                                "info",
                                "sync",
                                format!(
                                    "P2P {}: no getblocks after headers, pushing {} blocks [{}-{}]",
                                    fallback_addr,
                                    blocks.len(),
                                    start_height,
                                    end_h
                                ),
                            );
                            for block_data in blocks {
                                let msg = BlockPayload { block_data }.to_message();
                                let _ = send_message(&fallback_writer, msg, fallback_addr).await;
                            }
                        });
                    }
                }
            }
            MessageType::Ping => {
                if let Ok(ping) = PingPayload::from_message(&msg) {
                    let mut payload = Vec::new();
                    payload.extend_from_slice(&ping.nonce.to_be_bytes());
                    let pong = Message {
                        msg_type: MessageType::Pong,
                        payload,
                    };
                    let _ = send_message(&writer, pong, addr).await;
                    let mut dir_guard = directory.lock().await;
                    let multiaddr = format!("/ip4/{}/tcp/{}", addr.ip(), addr.port());
                    dir_guard.mark_seen(&multiaddr);
                }
            }
            MessageType::GetPeers => {
                let peers_payload = {
                    let dir = directory.lock().await;
                    PeersPayload {
                        peers: dir.peers().iter().map(|p| p.multiaddr.clone()).collect(),
                    }
                };
                if let Ok(resp) = peers_payload.to_message() {
                    let _ = send_message(&writer, resp, addr).await;
                }
            }
            MessageType::Peers => {
                if let Ok(list) = PeersPayload::from_message(&msg) {
                    let mut dir = directory.lock().await;
                    for p in list.peers {
                        dir.register_peer_hint(p);
                    }
                }
            }
            MessageType::GetHeaders => {
                if let Some(ref chain_arc) = chain {
                    if let Ok(payload) = GetHeadersPayload::from_message(&msg) {
                        let headers_bytes = {
                            let guard = chain_arc.lock().unwrap();

                            let mut start_idx = 0usize;
                            if !payload.start_hash.is_empty() && payload.start_hash.len() == 32 {
                                let mut target = [0u8; 32];
                                target.copy_from_slice(&payload.start_hash);
                                if let Some(pos) =
                                    guard.chain.iter().position(|b| b.header.hash() == target)
                                {
                                    start_idx = pos;
                                }
                            }

                            let mut bytes = Vec::new();
                            for block in guard
                                .chain
                                .iter()
                                .skip(start_idx)
                                .take(payload.count as usize)
                            {
                                bytes.extend_from_slice(&block.header.serialize());
                            }
                            bytes
                        };

                        let msg = HeadersPayload {
                            headers: headers_bytes,
                        }
                        .to_message();
                        let _ = send_message(&writer, msg, addr).await;
                    }
                }
            }
            MessageType::Headers => {
                if let Some(ref chain_arc) = chain {
                    if let Ok(payload) = HeadersPayload::from_message(&msg) {
                        let mut offset = 0usize;
                        while offset + 80 <= payload.headers.len() {
                            let slice = &payload.headers[offset..offset + 80];
                            let (header, used) = match crate::block::BlockHeader::deserialize(slice)
                            {
                                Ok(v) => v,
                                Err(e) => {
                                    eprintln!("Failed to parse header from {}: {}", addr, e);
                                    break;
                                }
                            };
                            offset += used;

                            {
                                let mut guard = chain_arc.lock().unwrap();
                                if let Err(e) = guard.add_header(header.clone()) {
                                    eprintln!("Header from {} rejected: {}", addr, e);
                                    break;
                                }
                            }
                        }

                        // After processing headers, check if a better-work fork exists and request its blocks.
                        if let Some(ref chain_arc) = chain {
                            let header_count = (payload.headers.len() / 80) as u32;
                            let request = {
                                let guard = chain_arc.lock().unwrap();
                                if let Some(best) = guard.best_header_if_better() {
                                    if let Some(path) =
                                        guard.header_path_to_known(best.header.hash())
                                    {
                                        if let Some(first_hash) = path.first() {
                                            let start_hash = guard
                                                .headers
                                                .get(first_hash)
                                                .map(|hw| hw.header.prev_hash)
                                                .unwrap_or([0u8; 32]);
                                            let count = path.len() as u32;
                                            if count > 0 {
                                                Some((start_hash, count))
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            };
                            let fallback = if request.is_none() && header_count > 0 {
                                Some((P2PNode::tip_hash(&chain), header_count))
                            } else {
                                None
                            };
                            if let Some((start_hash, count)) = request.or(fallback) {
                                let get_blocks = GetBlocksPayload {
                                    start_hash: start_hash.to_vec(),
                                    count,
                                };
                                if sync_block_request_allowed(&block_requests, addr.ip()).await {
                                    if let Ok(msg) = get_blocks.to_message() {
                                        let _ = send_message(&writer, msg, addr).await;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            MessageType::GetBlocks => {
                {
                    let mut guard = getblocks_seen.lock().await;
                    guard.insert(addr.ip(), Instant::now());
                }
                if let Some(ref chain_arc) = chain {
                    if let Ok(payload) = GetBlocksPayload::from_message(&msg) {
                        if !getblocks_request_allowed(
                            &getblocks_last,
                            addr.ip(),
                            &payload.start_hash,
                            payload.count,
                        ).await
                        {
                            continue;
                        }
                        let (blocks, start_height) = {
                            let guard = chain_arc.lock().unwrap();
                            let mut start_idx = 0usize;
                            if payload.start_hash.len() == 32 {
                                let mut target = [0u8; 32];
                                target.copy_from_slice(&payload.start_hash);
                                if let Some(pos) =
                                    guard.chain.iter().position(|b| b.header.hash() == target)
                                {
                                    start_idx = pos + 1;
                                }
                            }
                            let mut blocks = Vec::new();
                            let mut heights = Vec::new();
                            for (idx, b) in guard
                                .chain
                                .iter()
                                .enumerate()
                                .skip(start_idx)
                                .take(payload.count as usize)
                            {
                                blocks.push(b.serialize());
                                heights.push(idx as u64);
                            }
                            let start_h: u64 = heights.first().copied().unwrap_or(0);
                            (blocks, start_h)
                        };
                        if !blocks.is_empty() {
                            let end_h = start_height + blocks.len() as u64 - 1;
                            P2PNode::log(format!(
                                "P2P {}: sending {} blocks [{}-{}]",
                                addr,
                                blocks.len(),
                                start_height,
                                end_h
                            ));
                        }
                        for block_data in blocks {
                            let msg = BlockPayload { block_data }.to_message();
                            let _ = send_message(&writer, msg, addr).await;
                        }
                    }
                }
            }
            MessageType::Block => {
                if let Some(ref chain_arc) = chain {
                    if let Ok(payload) = BlockPayload::from_message(&msg) {
                        match Block::deserialize(&payload.block_data) {
                            Ok((block, _)) => {
                                let bhash = block.header.hash();
                                let short = hex::encode(bhash);
                                let short = short.get(0..12).unwrap_or(&short);
                                let (new_height_opt, record_verdict, persist_height) = {
                                    let mut guard = chain_arc.lock().unwrap();
                                    let mut new_height_opt = None;
                                    let mut record_verdict = None;
                                    let mut persist_height = None;
                                    match guard.process_block(block.clone()) {
                                        Ok((new_height, _tip)) => {
                                            P2PNode::log(format!(
                                                "P2P {}: accepted block height {} hash {}",
                                                addr, new_height.saturating_sub(1), short
                                            ));
                                            if let Some(ref mem) = mempool {
                                                let mut mem_guard = mem.lock().unwrap();
                                                for tx in block.transactions.iter().skip(1) {
                                                    mem_guard.remove(&tx.txid());
                                                }
                                            }
                                            // Update headers to reflect advanced tip.
                                            guard.headers.clear();
                                            guard.header_chain.clear();
                                            new_height_opt = Some(new_height);
                                            record_verdict = Some(true);
                                            if guard.tip_hash() == bhash {
                                                persist_height = Some(guard.tip_height());
                                            }
                                        }
                                        Err(e) => {
                                            if P2PNode::is_soft_block_reject(&e) {
                                                if !P2PNode::is_duplicate_block(&e) {
                                                    P2PNode::log_event(
                                                        "info",
                                                        "chain",
                                                        format!(
                                                            "P2P {}: ignored block {} ({})",
                                                            addr, short, e
                                                        ),
                                                    );
                                                }
                                            } else {
                                                P2PNode::log_event(
                                                    "warn",
                                                    "chain",
                                                    format!(
                                                        "P2P {}: rejected block {}: {}",
                                                        addr, short, e
                                                    ),
                                                );
                                                record_verdict = Some(false);
                                            }
                                        }
                                    }
                                    (new_height_opt, record_verdict, persist_height)
                                };
                                if let Some(height) = persist_height {
                                    if let Err(e) = storage::write_block_json(height, &block) {
                                        P2PNode::log_event(
                                            "warn",
                                            "chain",
                                            format!(
                                                "P2P {}: failed to persist block {}: {}",
                                                addr, short, e
                                            ),
                                        );
                                    }
                                }
                                if let Some(new_height) = new_height_opt {
                                    let multiaddr = format!("/ip4/{}/tcp/{}", addr.ip(), addr.port());
                                    let mut directory = directory.lock().await;
                                    directory.record_height(&multiaddr, new_height.saturating_sub(1));
                                }
                                if let Some(ok) = record_verdict {
                                    let mut rep = reputation.lock().await;
                                    rep.record_block(&addr.to_string(), ok);
                                }
                            }
                            Err(e) => {
                                eprintln!("Failed to decode block payload from {}: {}", addr, e);
                                let mut rep = reputation.lock().await;
                                rep.record_decode_error(&addr.to_string());
                            }
                        }
                    }
                }
            }
            MessageType::Tx => {
                if let Ok(payload) = TxPayload::from_message(&msg) {
                    match decode_full_tx(&payload.tx_data) {
                        Ok(tx) => {
                            if let (Some(ref chain_arc), Some(ref mem)) = (&chain, &mempool) {
                                let inv_bytes = {
                                    let fee = {
                                        let guard = chain_arc.lock().unwrap();
                                        match guard.calculate_fees(&tx) {
                                            Ok(f) => f,
                                            Err(e) => {
                                                eprintln!("Rejecting tx from {}: {}", addr, e);
                                                continue;
                                            }
                                        }
                                    };
                                    let relay_addr = {
                                        let dir = directory.lock().await;
                                        dir.relay_address_for_peer(&addr)
                                    };
                                    let mut mem_guard = mem.lock().unwrap();
                                    let peer_addr = addr.to_string();
                                    match mem_guard.add_transaction(
                                        tx.clone(),
                                        payload.tx_data.clone(),
                                        fee,
                                    ) {
                                        Ok(outcome) => {
                                            mem_guard
                                                .record_relay(&outcome.txid, peer_addr.clone());
                                        }
                                        Err(_) => {
                                            mem_guard.record_relay(&tx.txid(), peer_addr);
                                        }
                                    }
                                    if let Some(relay_addr) = relay_addr {
                                        mem_guard.record_relay_address(&tx.txid(), relay_addr);
                                    }
                                    InvPayload {
                                        txids: vec![hex::encode(tx.txid())],
                                    }
                                    .to_message()
                                    .ok()
                                    .map(|m| m.serialize())
                                };

                                if let Some(inv_bytes) = inv_bytes {
                                    let mut peers_guard = peers.lock().await;
                                    for socket in peers_guard.iter_mut() {
                                        let _ = socket.lock().await.write_all(&inv_bytes).await;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Failed to decode tx from {}: {}", addr, e);
                        }
                    }
                }
            }
            MessageType::Inv => {
                if let Some(ref mem) = mempool {
                    if let Ok(inv) = InvPayload::from_message(&msg) {
                        let mut needed = Vec::new();
                        {
                            let guard = mem.lock().unwrap();
                            for txid_hex in inv.txids {
                                if let Ok(bytes) = hex::decode(&txid_hex) {
                                    if bytes.len() == 32 {
                                        let mut txid = [0u8; 32];
                                        txid.copy_from_slice(&bytes);
                                        if !guard.contains(&txid) {
                                            needed.push(txid_hex.clone());
                                        }
                                    }
                                }
                            }
                        }
                        if !needed.is_empty() {
                            let gd = GetDataPayload { txids: needed };
                            if let Ok(msg) = gd.to_message() {
                                let _ = send_message(&writer, msg, addr).await;
                            }
                        }
                    }
                }
            }
            MessageType::GetData => {
                if let Some(ref mem) = mempool {
                    if let Ok(gd) = GetDataPayload::from_message(&msg) {
                        let mut responses: Vec<Message> = Vec::new();
                        {
                            let guard = mem.lock().unwrap();
                            for txid_hex in gd.txids {
                                if let Ok(bytes) = hex::decode(&txid_hex) {
                                    if bytes.len() != 32 {
                                        continue;
                                    }
                                    let mut txid = [0u8; 32];
                                    txid.copy_from_slice(&bytes);
                                    if let Some(raw) = guard.raw_tx(&txid) {
                                        responses.push(TxPayload { tx_data: raw }.to_message());
                                    }
                                }
                            }
                        }
                        for msg in responses {
                            let _ = send_message(&writer, msg, addr).await;
                        }
                    }
                }
            }
            MessageType::Mempool => {
                if let Some(ref mem) = mempool {
                    let tx_hashes: Vec<String> = {
                        let guard = mem.lock().unwrap();
                        guard.txids_hex()
                    };
                    let payload = MempoolPayload { tx_hashes };
                    if let Ok(msg) = payload.to_message() {
                        let _ = send_message(&writer, msg, addr).await;
                    }
                } else if let Ok(msg) = EmptyPayload::to_message(MessageType::Mempool) {
                    let _ = send_message(&writer, msg, addr).await;
                }
            }
            MessageType::RelayAddress => {
                if let Some(ref mem) = mempool {
                    if let Ok(relay) = RelayAddressPayload::from_message(&msg) {
                        if relay.txid.len() == 64 {
                            if let Ok(bytes) = hex::decode(relay.txid) {
                                if bytes.len() == 32 {
                                    let mut txid = [0u8; 32];
                                    txid.copy_from_slice(&bytes);
                                    let mut guard = mem.lock().unwrap();
                                    guard.record_relay_address(&txid, relay.address);
                                }
                            }
                        } else if relay.txid.is_empty() {
                            // Peer is advertising a default relay address; update directory mapping.
                            let multiaddr = format!("/ip4/{}/tcp/{}", addr.ip(), addr.port());
                            let mut dir = directory.lock().await;
                            dir.register_connection(multiaddr, None, Some(relay.address), None);
                        }
                    }
                }
            }
            MessageType::Disconnect => break,
            _ => {
                // Unhandled message types can be ignored for now.
            }
        }
    }
    {
        let mut guard = peers.lock().await;
        guard.retain(|p| !Arc::ptr_eq(p, &writer));
    }
    {
        let mut guard = connected.lock().await;
        if guard.remove(&addr) {
            P2PNode::log(format!("P2P inbound {}: disconnected", addr));
        }
    }
    Ok(())
}
