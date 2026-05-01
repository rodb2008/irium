use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

/// Protocol version for Irium P2P.
pub const PROTOCOL_VERSION: u8 = 1;
/// 32 MB max message size.
pub const MAX_MESSAGE_SIZE: u32 = 32 * 1024 * 1024;
/// 4 MB max block size.
pub const MAX_BLOCK_SIZE: u32 = 4 * 1024 * 1024;
/// Max headers per getheaders request.
pub const MAX_HEADERS_PER_REQUEST: u32 = 2000;
/// Max blocks per getblocks request.
pub const MAX_BLOCKS_PER_REQUEST: u32 = 512;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    Handshake = 1,
    Ping = 2,
    Pong = 3,
    GetPeers = 4,
    Peers = 5,
    GetBlocks = 6,
    Block = 7,
    GetHeaders = 8,
    Headers = 9,
    Tx = 10,
    Mempool = 11,
    SybilChallenge = 12,
    SybilProof = 13,
    RelayAddress = 14,
    Inv = 15,
    GetData = 16,
    UptimeChallenge = 17,
    UptimeProof = 18,
    ProofGossip = 19,
    Disconnect = 99,
}

impl TryFrom<u8> for MessageType {
    type Error = String;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        use MessageType::*;
        let mt = match value {
            1 => Handshake,
            2 => Ping,
            3 => Pong,
            4 => GetPeers,
            5 => Peers,
            6 => GetBlocks,
            7 => Block,
            8 => GetHeaders,
            9 => Headers,
            10 => Tx,
            11 => Mempool,
            12 => SybilChallenge,
            13 => SybilProof,
            14 => RelayAddress,
            15 => Inv,
            16 => GetData,
            17 => UptimeChallenge,
            18 => UptimeProof,
            19 => ProofGossip,
            99 => Disconnect,
            other => return Err(format!("Unknown message type: {}", other)),
        };
        Ok(mt)
    }
}

/// Base P2P message: [version:1][type:1][length:4][payload]
#[derive(Debug, Clone)]
pub struct Message {
    pub msg_type: MessageType,
    pub payload: Vec<u8>,
}

impl Message {
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(6 + self.payload.len());
        out.push(PROTOCOL_VERSION);
        out.push(self.msg_type as u8);
        let len = self.payload.len() as u32;
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(&self.payload);
        out
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, String> {
        if data.len() < 6 {
            return Err("Message too short".to_string());
        }
        let version = data[0];
        if version != PROTOCOL_VERSION {
            return Err(format!("Unsupported protocol version: {}", version));
        }
        let msg_type_byte = data[1];
        let mut len_bytes = [0u8; 4];
        len_bytes.copy_from_slice(&data[2..6]);
        let length = u32::from_be_bytes(len_bytes);
        if length > MAX_MESSAGE_SIZE {
            return Err("Message too large".to_string());
        }
        if data.len() < 6 + length as usize {
            return Err("Incomplete message".to_string());
        }
        let payload = data[6..6 + length as usize].to_vec();
        let msg_type = MessageType::try_from(msg_type_byte)?;
        Ok(Message { msg_type, payload })
    }
}

/// Handshake message with minimal fields (JSON payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakePayload {
    pub version: u32,
    pub agent: String,
    pub height: u64,
    pub timestamp: i64,
    pub port: u16,
    pub checkpoint_height: Option<u64>,
    pub checkpoint_hash: Option<String>,
    #[serde(default)]
    pub relay_address: Option<String>,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub tip_hash: Option<String>,
    #[serde(default)]
    pub capabilities: Option<Vec<String>>,
/// Optional URL of this node's marketplace offer feed (e.g. http://host:port/offers/feed).
    /// Propagated via P2P handshake so peers can discover feeds without manual configuration.
    #[serde(default)]
    pub marketplace_feed: Option<String>,
}

impl HandshakePayload {
    pub fn to_message(&self) -> Result<Message, String> {
        let json = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        Ok(Message {
            msg_type: MessageType::Handshake,
            payload: json,
        })
    }

    pub fn from_message(msg: &Message) -> Result<Self, String> {
        if msg.msg_type != MessageType::Handshake {
            return Err("Not a handshake message".to_string());
        }
        serde_json::from_slice(&msg.payload).map_err(|e| e.to_string())
    }
}

/// Ping message with 8-byte nonce.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingPayload {
    pub nonce: u64,
}
/// Gossip payload for settlement proofs. Raw JSON bytes of a SettlementProof.
pub struct ProofGossipPayload {
    pub proof_json: Vec<u8>,
}

impl ProofGossipPayload {
    pub fn to_message(&self) -> Message {
        Message {
            msg_type: MessageType::ProofGossip,
            payload: self.proof_json.clone(),
        }
    }

    pub fn from_message(msg: &Message) -> Result<Self, String> {
        if msg.msg_type != MessageType::ProofGossip {
            return Err("Not a proof gossip message".to_string());
        }
        Ok(ProofGossipPayload {
            proof_json: msg.payload.clone(),
        })
    }
}


impl PingPayload {
    pub fn to_message(&self) -> Message {
        let mut payload = Vec::with_capacity(8);
        payload.extend_from_slice(&self.nonce.to_be_bytes());
        Message {
            msg_type: MessageType::Ping,
            payload,
        }
    }

    pub fn from_message(msg: &Message) -> Result<Self, String> {
        if msg.msg_type != MessageType::Ping {
            return Err("Not a ping message".to_string());
        }
        if msg.payload.len() != 8 {
            return Err("Invalid ping payload length".to_string());
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&msg.payload);
        Ok(PingPayload {
            nonce: u64::from_be_bytes(bytes),
        })
    }
}

/// Pong message reuses PingPayload format.
pub type PongPayload = PingPayload;

/// Uptime challenge payload: nonce + timestamp.
#[derive(Debug, Clone)]
pub struct UptimeChallengePayload {
    pub nonce: [u8; 32],
    pub timestamp: u64,
}

impl UptimeChallengePayload {
    pub fn to_message(&self) -> Message {
        let mut payload = Vec::with_capacity(40);
        payload.extend_from_slice(&self.nonce);
        payload.extend_from_slice(&self.timestamp.to_be_bytes());
        Message {
            msg_type: MessageType::UptimeChallenge,
            payload,
        }
    }

    pub fn from_message(msg: &Message) -> Result<Self, String> {
        if msg.msg_type != MessageType::UptimeChallenge {
            return Err("Not an uptime challenge message".to_string());
        }
        if msg.payload.len() != 40 {
            return Err("Invalid uptime challenge payload length".to_string());
        }
        let mut nonce = [0u8; 32];
        nonce.copy_from_slice(&msg.payload[0..32]);
        let mut ts_bytes = [0u8; 8];
        ts_bytes.copy_from_slice(&msg.payload[32..40]);
        let timestamp = u64::from_be_bytes(ts_bytes);
        Ok(UptimeChallengePayload { nonce, timestamp })
    }
}

/// Uptime proof payload: nonce + timestamp + HMAC.
#[derive(Debug, Clone)]
pub struct UptimeProofPayload {
    pub nonce: [u8; 32],
    pub timestamp: u64,
    pub hmac: [u8; 32],
}

impl UptimeProofPayload {
    pub fn to_message(&self) -> Message {
        let mut payload = Vec::with_capacity(72);
        payload.extend_from_slice(&self.nonce);
        payload.extend_from_slice(&self.timestamp.to_be_bytes());
        payload.extend_from_slice(&self.hmac);
        Message {
            msg_type: MessageType::UptimeProof,
            payload,
        }
    }

    pub fn from_message(msg: &Message) -> Result<Self, String> {
        if msg.msg_type != MessageType::UptimeProof {
            return Err("Not an uptime proof message".to_string());
        }
        if msg.payload.len() != 72 {
            return Err("Invalid uptime proof payload length".to_string());
        }
        let nonce: [u8; 32] = msg.payload[0..32]
            .try_into()
            .map_err(|_| "Invalid uptime proof payload length".to_string())?;
        let ts_bytes: [u8; 8] = msg.payload[32..40]
            .try_into()
            .map_err(|_| "Invalid uptime proof payload length".to_string())?;
        let timestamp = u64::from_be_bytes(ts_bytes);
        let hmac: [u8; 32] = msg.payload[40..72]
            .try_into()
            .map_err(|_| "Invalid uptime proof payload length".to_string())?;
        Ok(UptimeProofPayload {
            nonce,
            timestamp,
            hmac,
        })
    }
}

/// Block message: raw block bytes.
#[derive(Debug, Clone)]
pub struct BlockPayload {
    pub block_data: Vec<u8>,
}

impl BlockPayload {
    pub fn to_message(&self) -> Message {
        Message {
            msg_type: MessageType::Block,
            payload: self.block_data.clone(),
        }
    }

    pub fn from_message(msg: &Message) -> Result<Self, String> {
        if msg.msg_type != MessageType::Block {
            return Err("Not a block message".to_string());
        }
        if msg.payload.len() as u32 > MAX_BLOCK_SIZE {
            return Err("Block payload too large".to_string());
        }
        Ok(BlockPayload {
            block_data: msg.payload.clone(),
        })
    }
}

/// Tx message: raw transaction bytes.
#[derive(Debug, Clone)]
pub struct TxPayload {
    pub tx_data: Vec<u8>,
}

impl TxPayload {
    pub fn to_message(&self) -> Message {
        Message {
            msg_type: MessageType::Tx,
            payload: self.tx_data.clone(),
        }
    }

    pub fn from_message(msg: &Message) -> Result<Self, String> {
        if msg.msg_type != MessageType::Tx {
            return Err("Not a tx message".to_string());
        }
        Ok(TxPayload {
            tx_data: msg.payload.clone(),
        })
    }
}

/// Empty payload helper for messages without a body.
#[derive(Debug, Clone)]
pub struct EmptyPayload;

impl EmptyPayload {
    pub fn to_message(msg_type: MessageType) -> Result<Message, String> {
        Ok(Message {
            msg_type,
            payload: Vec::new(),
        })
    }
}

/// Peers message: JSON list of multiaddrs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeersPayload {
    pub peers: Vec<String>,
}

impl PeersPayload {
    pub fn to_message(&self) -> Result<Message, String> {
        let json = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        Ok(Message {
            msg_type: MessageType::Peers,
            payload: json,
        })
    }

    pub fn from_message(msg: &Message) -> Result<Self, String> {
        if msg.msg_type != MessageType::Peers {
            return Err("Not a peers message".to_string());
        }
        serde_json::from_slice(&msg.payload).map_err(|e| e.to_string())
    }
}

/// Request block bodies starting from a given hash.
#[derive(Debug, Clone)]
pub struct GetBlocksPayload {
    pub start_hash: Vec<u8>,
    pub count: u32,
}

impl GetBlocksPayload {
    pub fn to_message(&self) -> Result<Message, String> {
        let mut payload = Vec::with_capacity(4 + self.start_hash.len());
        payload.extend_from_slice(&self.count.to_be_bytes());
        payload.extend_from_slice(&self.start_hash);
        Ok(Message {
            msg_type: MessageType::GetBlocks,
            payload,
        })
    }

    pub fn from_message(msg: &Message) -> Result<Self, String> {
        if msg.msg_type != MessageType::GetBlocks {
            return Err("Not a get_blocks message".to_string());
        }
        if msg.payload.len() < 4 {
            return Err("get_blocks payload too short".to_string());
        }
        let mut count_bytes = [0u8; 4];
        count_bytes.copy_from_slice(&msg.payload[0..4]);
        let count = u32::from_be_bytes(count_bytes);
        let start_hash = msg.payload[4..].to_vec();
        if !start_hash.is_empty() && start_hash.len() != 32 {
            return Err("get_blocks start_hash invalid".to_string());
        }
        if count == 0 || count > MAX_BLOCKS_PER_REQUEST {
            return Err("get_blocks count out of range".to_string());
        }
        Ok(GetBlocksPayload { start_hash, count })
    }
}

/// Request block headers (SPV/light clients).
#[derive(Debug, Clone)]
pub struct GetHeadersPayload {
    pub start_hash: Vec<u8>,
    pub count: u32,
}

impl GetHeadersPayload {
    pub fn to_message(&self) -> Result<Message, String> {
        let mut payload = Vec::with_capacity(4 + self.start_hash.len());
        payload.extend_from_slice(&self.count.to_be_bytes());
        payload.extend_from_slice(&self.start_hash);
        Ok(Message {
            msg_type: MessageType::GetHeaders,
            payload,
        })
    }

    pub fn from_message(msg: &Message) -> Result<Self, String> {
        if msg.msg_type != MessageType::GetHeaders {
            return Err("Not a get_headers message".to_string());
        }
        if msg.payload.len() < 4 {
            return Err("get_headers payload too short".to_string());
        }
        let mut count_bytes = [0u8; 4];
        count_bytes.copy_from_slice(&msg.payload[0..4]);
        let count = u32::from_be_bytes(count_bytes);
        let start_hash = msg.payload[4..].to_vec();
        if !start_hash.is_empty() && start_hash.len() != 32 {
            return Err("get_headers start_hash invalid".to_string());
        }
        if count == 0 || count > MAX_HEADERS_PER_REQUEST {
            return Err("get_headers count out of range".to_string());
        }
        Ok(GetHeadersPayload { start_hash, count })
    }
}

/// Headers message: raw concatenated headers.
#[derive(Debug, Clone)]
pub struct HeadersPayload {
    pub headers: Vec<u8>,
}

impl HeadersPayload {
    pub fn to_message(&self) -> Message {
        Message {
            msg_type: MessageType::Headers,
            payload: self.headers.clone(),
        }
    }

    pub fn from_message(msg: &Message) -> Result<Self, String> {
        if msg.msg_type != MessageType::Headers {
            return Err("Not a headers message".to_string());
        }
        Ok(HeadersPayload {
            headers: msg.payload.clone(),
        })
    }
}

/// Mempool message: JSON list of tx hashes in hex.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolPayload {
    pub tx_hashes: Vec<String>,
}

impl MempoolPayload {
    pub fn to_message(&self) -> Result<Message, String> {
        let json = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        Ok(Message {
            msg_type: MessageType::Mempool,
            payload: json,
        })
    }

    pub fn from_message(msg: &Message) -> Result<Self, String> {
        if msg.msg_type != MessageType::Mempool {
            return Err("Not a mempool message".to_string());
        }
        serde_json::from_slice(&msg.payload).map_err(|e| e.to_string())
    }
}

/// Optional relay address announcement for a transaction: peer can share
/// a payout address to be considered for relay rewards.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayAddressPayload {
    pub txid: String,
    pub address: String,
}

impl RelayAddressPayload {
    pub fn to_message(&self) -> Result<Message, String> {
        let json = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        Ok(Message {
            msg_type: MessageType::RelayAddress,
            payload: json,
        })
    }

    pub fn from_message(msg: &Message) -> Result<Self, String> {
        if msg.msg_type != MessageType::RelayAddress {
            return Err("Not a relay address message".to_string());
        }
        serde_json::from_slice(&msg.payload).map_err(|e| e.to_string())
    }
}

/// Inventory announcement (txids only for now).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvPayload {
    pub txids: Vec<String>,
}

impl InvPayload {
    pub fn to_message(&self) -> Result<Message, String> {
        let json = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        Ok(Message {
            msg_type: MessageType::Inv,
            payload: json,
        })
    }

    pub fn from_message(msg: &Message) -> Result<Self, String> {
        if msg.msg_type != MessageType::Inv {
            return Err("Not an inv message".to_string());
        }
        serde_json::from_slice(&msg.payload).map_err(|e| e.to_string())
    }
}

/// Request specific data by txid (txids only for now).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetDataPayload {
    pub txids: Vec<String>,
}

impl GetDataPayload {
    pub fn to_message(&self) -> Result<Message, String> {
        let json = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        Ok(Message {
            msg_type: MessageType::GetData,
            payload: json,
        })
    }

    pub fn from_message(msg: &Message) -> Result<Self, String> {
        if msg.msg_type != MessageType::GetData {
            return Err("Not a getdata message".to_string());
        }
        serde_json::from_slice(&msg.payload).map_err(|e| e.to_string())
    }
}

/// Disconnect message with UTF-8 reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisconnectPayload {
    pub reason: String,
}

impl DisconnectPayload {
    pub fn to_message(&self) -> Message {
        Message {
            msg_type: MessageType::Disconnect,
            payload: self.reason.as_bytes().to_vec(),
        }
    }

    pub fn from_message(msg: &Message) -> Result<Self, String> {
        if msg.msg_type != MessageType::Disconnect {
            return Err("Not a disconnect message".to_string());
        }
        let reason = String::from_utf8(msg.payload.clone()).map_err(|e| e.to_string())?;
        Ok(DisconnectPayload { reason })
    }
}
