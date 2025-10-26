"""Sybil-resistant handshake for Irium P2P network."""

from __future__ import annotations
import hashlib
import hmac
import secrets
import time
from dataclasses import dataclass
from typing import Optional


@dataclass
class SybilChallenge:
    """Challenge for sybil-resistant handshake."""
    
    nonce: bytes
    timestamp: int
    difficulty: int
    
    @classmethod
    def create(cls, difficulty: int = 8):
        """Create a new challenge."""
        return cls(
            nonce=secrets.token_bytes(32),
            timestamp=int(time.time()),
            difficulty=difficulty
        )
    
    def to_bytes(self) -> bytes:
        """Serialize challenge."""
        return self.nonce + self.timestamp.to_bytes(8, 'big') + bytes([self.difficulty])
    
    @classmethod
    def from_bytes(cls, data: bytes):
        """Deserialize challenge."""
        nonce = data[:32]
        timestamp = int.from_bytes(data[32:40], 'big')
        difficulty = data[40]
        return cls(nonce=nonce, timestamp=timestamp, difficulty=difficulty)


@dataclass
class SybilProof:
    """Proof-of-work for sybil resistance."""
    
    challenge: SybilChallenge
    solution: int
    peer_pubkey: bytes
    
    def verify(self) -> bool:
        """Verify the proof-of-work."""
        data = (
            self.challenge.to_bytes() + 
            self.solution.to_bytes(8, 'big') +
            self.peer_pubkey
        )
        hash_result = hashlib.sha256(data).digest()
        required_zeros = self.challenge.difficulty
        hash_int = int.from_bytes(hash_result, 'big')
        return hash_int < (2 ** (256 - required_zeros))
    
    @classmethod
    def solve(cls, challenge: SybilChallenge, peer_pubkey: bytes):
        """Solve the proof-of-work challenge."""
        solution = 0
        
        while True:
            proof = cls(
                challenge=challenge,
                solution=solution,
                peer_pubkey=peer_pubkey
            )
            
            if proof.verify():
                return proof
            
            solution += 1
            
            if solution > 1000000:
                raise ValueError("Could not solve challenge")
    
    def to_bytes(self) -> bytes:
        """Serialize proof."""
        return (
            self.challenge.to_bytes() +
            self.solution.to_bytes(8, 'big') +
            self.peer_pubkey
        )
    
    @classmethod
    def from_bytes(cls, data: bytes):
        """Deserialize proof."""
        challenge = SybilChallenge.from_bytes(data[:41])
        solution = int.from_bytes(data[41:49], 'big')
        peer_pubkey = data[49:]
        return cls(challenge=challenge, solution=solution, peer_pubkey=peer_pubkey)


class SybilResistantHandshake:
    """Sybil-resistant handshake protocol."""
    
    def __init__(self, difficulty: int = 8):
        self.difficulty = difficulty
    
    def create_challenge(self):
        """Create handshake challenge."""
        return SybilChallenge.create(difficulty=self.difficulty)
    
    def verify_proof(self, proof: SybilProof) -> bool:
        """Verify handshake proof."""
        age = time.time() - proof.challenge.timestamp
        if age > 300:
            return False
        return proof.verify()
    
    def solve_challenge(self, challenge: SybilChallenge, peer_pubkey: bytes):
        """Solve handshake challenge."""
        return SybilProof.solve(challenge, peer_pubkey)


# Legacy compatibility
def generate_uptime_token():
    """Generate uptime token (legacy)."""
    return secrets.token_bytes(32)


def verify_uptime_token(token: bytes) -> bool:
    """Verify uptime token (legacy)."""
    return len(token) == 32


class HandshakeToken:
    """Legacy handshake token."""
    def __init__(self):
        self.token = generate_uptime_token()
