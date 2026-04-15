use rand_core::OsRng;
use rand_core::RngCore;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

/// Challenge for sybil-resistant handshake.
#[derive(Debug, Clone)]
pub struct SybilChallenge {
    pub nonce: [u8; 32],
    pub timestamp: u64,
    pub difficulty: u8,
}

impl SybilChallenge {
    /// Create a new challenge with the given difficulty (in leading bits).
    pub fn create(difficulty: u8) -> SybilChallenge {
        let mut nonce = [0u8; 32];
        OsRng.fill_bytes(&mut nonce);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        SybilChallenge {
            nonce,
            timestamp,
            difficulty,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + 8 + 1);
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.timestamp.to_be_bytes());
        out.push(self.difficulty);
        out
    }

    pub fn from_bytes(data: &[u8]) -> Option<SybilChallenge> {
        if data.len() < 41 {
            return None;
        }
        let mut nonce = [0u8; 32];
        nonce.copy_from_slice(&data[..32]);
        let mut ts_bytes = [0u8; 8];
        ts_bytes.copy_from_slice(&data[32..40]);
        let timestamp = u64::from_be_bytes(ts_bytes);
        let difficulty = data[40];
        Some(SybilChallenge {
            nonce,
            timestamp,
            difficulty,
        })
    }
}

/// Proof-of-work for sybil resistance.
#[derive(Debug, Clone)]
pub struct SybilProof {
    pub challenge: SybilChallenge,
    pub solution: u64,
    pub peer_pubkey: Vec<u8>,
}

impl SybilProof {
    pub fn verify(&self) -> bool {
        let mut data = Vec::new();
        data.extend_from_slice(&self.challenge.to_bytes());
        data.extend_from_slice(&self.solution.to_be_bytes());
        data.extend_from_slice(&self.peer_pubkey);

        let hash_result = Sha256::digest(&data);
        leading_zero_bits(&hash_result) >= self.challenge.difficulty as u32
    }

    pub fn solve(challenge: SybilChallenge, peer_pubkey: Vec<u8>) -> Result<SybilProof, String> {
        let mut solution: u64 = 0;
        loop {
            let proof = SybilProof {
                challenge: challenge.clone(),
                solution,
                peer_pubkey: peer_pubkey.clone(),
            };
            if proof.verify() {
                return Ok(proof);
            }
            solution = solution
                .checked_add(1)
                .ok_or_else(|| "solution counter overflow".to_string())?;
            if solution > 50_000_000 {
                return Err("Could not solve challenge within 50,000,000 iterations".to_string());
            }
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.challenge.to_bytes());
        out.extend_from_slice(&self.solution.to_be_bytes());
        out.extend_from_slice(&self.peer_pubkey);
        out
    }

    pub fn from_bytes(data: &[u8]) -> Option<SybilProof> {
        if data.len() < 41 + 8 {
            return None;
        }
        let challenge = SybilChallenge::from_bytes(&data[..41])?;
        let mut sol_bytes = [0u8; 8];
        sol_bytes.copy_from_slice(&data[41..49]);
        let solution = u64::from_be_bytes(sol_bytes);
        let peer_pubkey = data[49..].to_vec();
        Some(SybilProof {
            challenge,
            solution,
            peer_pubkey,
        })
    }
}

/// Count leading zero bits in a byte slice.
fn leading_zero_bits(bytes: &[u8]) -> u32 {
    let mut count = 0u32;
    for b in bytes {
        if *b == 0 {
            count += 8;
            continue;
        }
        count += (*b).leading_zeros();
        break;
    }
    count
}

/// High-level helper mirroring `SybilResistantHandshake` in Python.
pub struct SybilResistantHandshake {
    pub difficulty: u8,
}

impl SybilResistantHandshake {
    pub fn new(difficulty: u8) -> SybilResistantHandshake {
        SybilResistantHandshake { difficulty }
    }

    pub fn create_challenge(&self) -> SybilChallenge {
        SybilChallenge::create(self.difficulty)
    }

    pub fn verify_proof(&self, proof: &SybilProof) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let age = now.saturating_sub(proof.challenge.timestamp);
        if age > 300 {
            return false;
        }
        proof.verify()
    }

    pub fn solve_challenge(
        &self,
        challenge: SybilChallenge,
        peer_pubkey: Vec<u8>,
    ) -> Result<SybilProof, String> {
        SybilProof::solve(challenge, peer_pubkey)
    }
}
