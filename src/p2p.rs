use std::net::SocketAddr;
use std::sync::{Arc, Mutex as StdMutex};

use chrono::Utc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{tcp::OwnedWriteHalf, TcpListener, TcpStream};
use tokio::sync::Mutex;

use crate::block::Block;
use crate::chain::ChainState;
use crate::mempool::MempoolManager;
use crate::network::PeerDirectory;
use crate::protocol::{
    BlockPayload, EmptyPayload, GetBlocksPayload, GetDataPayload, GetHeadersPayload,
    HandshakePayload, HeadersPayload, InvPayload, MempoolPayload, Message, MessageType,
    PeersPayload, PingPayload, RelayAddressPayload, TxPayload, MAX_MESSAGE_SIZE,
};
use crate::reputation::ReputationManager;
use crate::sybil::{SybilChallenge, SybilProof, SybilResistantHandshake};
use crate::tx::decode_full_tx;

/// Minimal P2P node skeleton: accepts incoming connections and can
/// broadcast raw block bytes to all connected peers.
#[derive(Clone)]
pub struct P2PNode {
    bind_addr: SocketAddr,
    peers: Arc<Mutex<Vec<Arc<Mutex<OwnedWriteHalf>>>>>,
    peers_directory: Arc<Mutex<PeerDirectory>>,
    reputation: Arc<Mutex<ReputationManager>>,
    chain: Option<Arc<StdMutex<ChainState>>>,
    mempool: Option<Arc<StdMutex<MempoolManager>>>,
    agent: String,
    relay_address: Option<String>,
}

impl P2PNode {
    fn tip_hash(chain: &Option<Arc<StdMutex<ChainState>>>) -> [u8; 32] {
        if let Some(ref c) = chain {
            let guard = c.lock().unwrap();
            if let Some(last) = guard.chain.last() {
                return last.header.hash();
            }
        }
        [0u8; 32]
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
            reputation: Arc::new(Mutex::new(ReputationManager::new())),
            chain,
            mempool,
            agent,
            relay_address,
        }
    }

    /// Start listening for incoming peers. This is a basic skeleton and
    /// performs a basic sybil-resistant handshake before accepting peers.
    pub async fn start(&self) -> Result<(), String> {
        let listener = TcpListener::bind(self.bind_addr)
            .await
            .map_err(|e| e.to_string())?;
        println!("P2P listening on {}", self.bind_addr);

        let peers_arc = self.peers.clone();
        let bind = self.bind_addr;
        let dir_arc = self.peers_directory.clone();
        let rep_arc = self.reputation.clone();
        let chain = self.chain.clone();
        let mempool = self.mempool.clone();
        let agent = self.agent.clone();
        let relay_address = self.relay_address.clone();

        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((socket, addr)) => {
                        println!("Incoming P2P connection from {}", addr);
                        let peers_inner = peers_arc.clone();
                        let dir = dir_arc.clone();
                        let rep = rep_arc.clone();
                        let chain_peer = chain.clone();
                        let mempool_peer = mempool.clone();
                        let agent_peer = agent.clone();
                        let relay_peer = relay_address.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_incoming_with_sybil(
                                socket,
                                addr,
                                bind,
                                peers_inner,
                                dir.clone(),
                                rep.clone(),
                                chain_peer,
                                mempool_peer,
                                agent_peer,
                                relay_peer,
                            )
                            .await
                            {
                                eprintln!("P2P handshake error from {}: {}", addr, e);
                            }
                        });
                    }
                    Err(e) => {
                        eprintln!("P2P accept error: {}", e);
                    }
                }
            }
        });

        Ok(())
    }

    /// Broadcast a raw serialized block to all currently known peers.
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

        println!("P2P outbound {}: connected, awaiting challenge", addr);
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

        // For now, use a fixed 32-byte token as the peer "pubkey"
        // for the purposes of sybil proof binding.
        let peer_pubkey = vec![0u8; 32];
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
        println!("P2P outbound {}: sent sybil proof", addr);

        let payload = HandshakePayload {
            version: 1,
            agent: agent.to_string(),
            height: local_height,
            timestamp: Utc::now().timestamp(),
            port: self.bind_addr.port(),
            checkpoint_height: None,
            checkpoint_hash: None,
            relay_address: self.relay_address.clone(),
        };

        let msg = payload
            .to_message()
            .map_err(|e| format!("build handshake message failed: {}", e))?;
        let bytes = msg.serialize();

        stream
            .write_all(&bytes)
            .await
            .map_err(|e| format!("send handshake to {} failed: {}", addr, e))?;
        println!("P2P outbound {}: sent handshake", addr);

        let (mut reader, writer_half) = stream.into_split();
        let writer = Arc::new(tokio::sync::Mutex::new(writer_half));

        {
            let mut guard = self.peers.lock().await;
            guard.push(writer.clone());
        }

        {
            let mut dir = self.peers_directory.lock().await;
            let multiaddr = format!("/ip4/{}/tcp/{}", addr.ip(), addr.port());
            dir.register_connection(multiaddr, None, self.relay_address.clone());
        }

        let dir = self.peers_directory.clone();
        let relay_addr = self.relay_address.clone();
        let chain_for_sync = self.chain.clone();
        let _mempool_for_sync = self.mempool.clone();
        tokio::spawn(async move {
            loop {
                match read_message(&mut reader).await {
                    Ok(msg) => match msg.msg_type {
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
                                let mut dir_guard = dir.lock().await;
                                dir_guard.register_connection(
                                    format!("/ip4/{}/tcp/{}", addr.ip(), addr.port()),
                                    Some(agent_str.clone()),
                                    payload.relay_address.clone(),
                                );
                                println!(
                                    "P2P outbound {}: received handshake (agent {}, height {})",
                                    addr, agent_str, payload.height
                                );
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
                    },
                    Err(e) => {
                        println!("P2P outbound {}: closing read loop: {}", addr, e);
                        let mut rep = reputation.lock().await;
                        rep.record_failure(&addr.to_string());
                        break;
                    }
                }
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
    directory: Arc<Mutex<PeerDirectory>>,
    reputation: Arc<Mutex<ReputationManager>>,
    chain: Option<Arc<StdMutex<ChainState>>>,
    mempool: Option<Arc<StdMutex<MempoolManager>>>,
    agent: String,
    relay_address: Option<String>,
) -> Result<(), String> {
    // Issue a fresh challenge with default difficulty 8.
    let handshake = SybilResistantHandshake::new(8);
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
    let proof_msg = read_message(&mut socket).await?;
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
        dir.register_connection(multiaddr, None, None);
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
    };
    if let Ok(msg) = payload.to_message() {
        let _ = send_message(&writer, msg, addr).await;
    }

    // Process messages from the peer.
    loop {
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
                    };
                    if let Ok(handshake_msg) = response.to_message() {
                        let _ = send_message(&writer, handshake_msg, addr).await;
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
                        dir.register_connection(p, None, None);
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
                                    if let Some(path) = guard.header_path_to_known(best.header.hash()) {
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
                            dir.register_connection(multiaddr, None, Some(relay.address));
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

    Ok(())
}
