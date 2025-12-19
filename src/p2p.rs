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
use crate::mempool::MempoolManager;
use crate::network::{PeerDirectory, PeerRecord};
use crate::protocol::{
    BlockPayload, EmptyPayload, GetBlocksPayload, GetDataPayload, GetHeadersPayload,
    HandshakePayload, HeadersPayload, InvPayload, MempoolPayload, Message, MessageType,
    PeersPayload, PingPayload, RelayAddressPayload, TxPayload, MAX_MESSAGE_SIZE,
};
use crate::reputation::ReputationManager;
use crate::sybil::{SybilChallenge, SybilProof, SybilResistantHandshake};
use crate::tx::decode_full_tx;
use rand_core::{OsRng, RngCore};
use serde_json::json;

/// Minimal P2P node skeleton: accepts incoming connections and can
/// broadcast raw block bytes to all connected peers.
const MAX_PEERS: usize = 100;
const MAX_MSGS_PER_SEC: u32 = 200;

#[derive(Clone)]
pub struct P2PNode {
    bind_addr: SocketAddr,
    peers: Arc<Mutex<Vec<Arc<Mutex<OwnedWriteHalf>>>>>,
    peers_directory: Arc<Mutex<PeerDirectory>>,
    connected: Arc<Mutex<HashSet<SocketAddr>>>,
    reputation: Arc<Mutex<ReputationManager>>,
    accept_log: Arc<Mutex<HashMap<IpAddr, Instant>>>,
    handshake_failures: Arc<StdMutex<HashMap<IpAddr, (u32, Instant)>>>,
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

    fn log(msg: impl AsRef<str>) {
        if Self::json_log_enabled() {
            let payload = json!({"ts": Self::ts(), "level": "info", "msg": msg.as_ref()});
            println!("{}", payload);
        } else {
            println!("[{}] {}", Self::ts(), msg.as_ref());
        }
    }

    fn log_err(msg: impl AsRef<str>) {
        if Self::json_log_enabled() {
            let payload = json!({"ts": Self::ts(), "level": "error", "msg": msg.as_ref()});
            eprintln!("{}", payload);
        } else {
            eprintln!("[{}] {}", Self::ts(), msg.as_ref());
        }
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
            handshake_failures: Arc::new(StdMutex::new(HashMap::new())),
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
        let handshake_failures = self.handshake_failures.clone();
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
                        if current >= MAX_PEERS {
                            Self::log_err(format!("Rejecting inbound {}: max peers reached", addr));
                            continue;
                        }
                        Self::log(format!("Incoming P2P connection from {}", addr));
                        let handshake_failures_task = handshake_failures.clone();
                        let dynamic_bans_task = dynamic_bans.clone();
                        let peers_inner = peers_arc.clone();
                        let connected_inner = connected.clone();
                        let dir = dir_arc.clone();
                        let rep = rep_arc.clone();
                        let chain_peer = chain.clone();
                        let mempool_peer = mempool.clone();
                        let agent_peer = agent.clone();
                        let relay_peer = relay_address.clone();
                        let node_id_peer = node_id.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_incoming_with_sybil(
                                socket,
                                addr,
                                bind,
                                peers_inner,
                                connected_inner.clone(),
                                dir.clone(),
                                rep.clone(),
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
        dir.peers()
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

    pub async fn broadcast_block(&self, block_bytes: &[u8]) -> Result<(), String> {
        let msg = BlockPayload {
            block_data: block_bytes.to_vec(),
        }
        .to_message();
        let serialized = msg.serialize();

        let mut guard = self.peers.lock().await;
        for socket in guard.iter_mut() {
            if let Err(e) = socket.lock().await.write_all(&serialized).await {
                eprintln!("Failed to send block to peer: {}", e);
            }
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
        local_height: u64,
        agent: &str,
    ) -> Result<(), String> {
        if self.is_connected(&addr).await {
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
        let challenge_msg = match read_message(&mut stream).await {
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
            height: local_height,
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

        let dir = self.peers_directory.clone();
        let relay_addr = self.relay_address.clone();
        let chain_for_sync = self.chain.clone();
        let _mempool_for_sync = self.mempool.clone();
        let reputation = self.reputation.clone();
        let peers_vec = self.peers.clone();
        let connected_vec = self.connected.clone();
        let writer_for_drop = writer.clone();
        tokio::spawn(async move {
            let mut msg_count: u32 = 0;
            let mut window_start = Instant::now();
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
                match read_message(&mut reader).await {
                    Ok(msg) => {
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
                                }
                            }
                            MessageType::Handshake => {
                                if let Ok(payload) = HandshakePayload::from_message(&msg) {
                                    let agent_str = payload.agent.clone();
                                    let node_id = payload.node_id.clone();
                                    {
                                        let mut dir_guard = dir.lock().await;
                                        dir_guard.register_connection(
                                            format!("/ip4/{}/tcp/{}", addr.ip(), addr.port()),
                                            Some(agent_str.clone()),
                                            payload.relay_address.clone(),
                                            node_id.clone(),
                                        );
                                    }
                                    Self::log(format!(
                                        "P2P outbound {}: received handshake (agent {}, height {})",
                                        addr, agent_str, payload.height
                                    ));
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
                                    if payload.height > local_height {
                                        let start_hash = P2PNode::tip_hash(&chain_for_sync);
                                        let get_blocks = GetBlocksPayload {
                                            start_hash: start_hash.to_vec(),
                                            count: 512,
                                        };
                                        if let Ok(msg) = get_blocks.to_message() {
                                            let _ = send_message(&writer, msg, addr).await;
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
        .and_then(|c| c.lock().ok().map(|g| g.height))
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
    chain: Option<Arc<StdMutex<ChainState>>>,
    mempool: Option<Arc<StdMutex<MempoolManager>>>,
    agent: String,
    relay_address: Option<String>,
    node_id: Vec<u8>,
) -> Result<(), String> {
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
    let proof_msg = match read_message(&mut socket).await {
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

    // Register the peer in the directory for future runtime seedlist updates.
    {
        let mut dir = directory.lock().await;
        let multiaddr = format!("/ip4/{}/tcp/{}", addr.ip(), addr.port());
        let node_id = hex::encode(&proof.peer_pubkey);
        dir.register_connection(multiaddr, None, None, Some(node_id));
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
        let msg = match read_message(&mut reader).await {
            Ok(m) => m,
            Err(e) => return Err(e),
        };

        match msg.msg_type {
            MessageType::Handshake => {
                if let Ok(payload) = HandshakePayload::from_message(&msg) {
                    let advertised_port = if payload.port > 0 {
                        payload.port
                    } else {
                        bind_addr.port()
                    };
                    let multiaddr = format!("/ip4/{}/tcp/{}", addr.ip(), advertised_port);
                    {
                        let mut dir = directory.lock().await;
                        dir.register_connection(
                            multiaddr,
                            Some(payload.agent.clone()),
                            payload.relay_address.clone(),
                            payload.node_id.clone(),
                        );
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
                        let start_hash = P2PNode::tip_hash(&chain);
                        let get_headers = GetHeadersPayload {
                            start_hash: start_hash.to_vec(),
                            count: 64,
                        };
                        if let Ok(msg) = get_headers.to_message() {
                            let _ = send_message(&writer, msg, addr).await;
                        }
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
                        dir.register_connection(p, None, None, None);
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

                            let mut maybe_request: Option<Message> = None;
                            {
                                let mut guard = chain_arc.lock().unwrap();
                                match guard.add_header(header.clone()) {
                                    Ok(h) => {
                                        // Only request body if it extends the current main tip.
                                        if guard.connects_to_tip(&header) {
                                            let get_blocks = GetBlocksPayload {
                                                start_hash: header.prev_hash.to_vec(),
                                                count: 1,
                                            };
                                            maybe_request = get_blocks.to_message().ok();
                                        } else {
                                            // Header added to tree but not current tip; skip body for now.
                                            let _ = h;
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("Header from {} rejected: {}", addr, e);
                                        break;
                                    }
                                }
                            }
                            if let Some(msg) = maybe_request.take() {
                                let _ = send_message(&writer, msg, addr).await;
                            }
                        }

                        // After processing headers, check if a better-work fork exists and request its blocks.
                        if let Some(ref chain_arc) = chain {
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
                            if let Some((start_hash, count)) = request {
                                let get_blocks = GetBlocksPayload {
                                    start_hash: start_hash.to_vec(),
                                    count,
                                };
                                if let Ok(msg) = get_blocks.to_message() {
                                    let _ = send_message(&writer, msg, addr).await;
                                }
                            }
                        }
                    }
                }
            }
            MessageType::GetBlocks => {
                if let Some(ref chain_arc) = chain {
                    if let Ok(payload) = GetBlocksPayload::from_message(&msg) {
                        let blocks: Vec<Vec<u8>> = {
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
                            guard
                                .chain
                                .iter()
                                .skip(start_idx)
                                .take(payload.count as usize)
                                .map(|b| b.serialize())
                                .collect()
                        };
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
                                let ok = {
                                    let mut guard = chain_arc.lock().unwrap();
                                    match guard.process_block(block.clone()) {
                                        Ok((_h, _tip)) => {
                                            if let Some(ref mem) = mempool {
                                                let mut mem_guard = mem.lock().unwrap();
                                                for tx in block.transactions.iter().skip(1) {
                                                    mem_guard.remove(&tx.txid());
                                                }
                                            }
                                            // Update headers to reflect advanced tip.
                                            guard.headers.clear();
                                            guard.header_chain.clear();
                                            true
                                        }
                                        Err(e) => {
                                            eprintln!(
                                                "Rejecting block from {} during P2P sync: {}",
                                                addr, e
                                            );
                                            false
                                        }
                                    }
                                };
                                let mut rep = reputation.lock().await;
                                rep.record_block(&addr.to_string(), ok);
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
    Ok(())
}
