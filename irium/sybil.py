"""Sybil-resistant handshake for Irium P2P network."""

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
    difficulty: int  # Required leading zero bits
    
    @classmethod
    def create(cls, difficulty: int = 8) -> SybilChallenge:
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
    def from_bytes(cls, data: bytes) -> SybilChallenge:
        """Deserialize challenge."""
        nonce = data[:32]
        timestamp = int.from_bytes(data[32:40], 'big')
        difficulty = data[40]
        return cls(nonce=nonce, timestamp=timestamp, difficulty=difficulty)


@dataclass
class SybilProof:
    """Proof-of-work for sybil resistance."""
    
    challenge: SybilChallenge
    solution: int  # Nonce that satisfies difficulty
    peer_pubkey: bytes  # Peer's public key
    
    def verify(self) -> bool:
        """Verify the proof-of-work."""
        # Compute hash
        data = (
            self.challenge.to_bytes() + 
            self.solution.to_bytes(8, 'big') +
            self.peer_pubkey
        )
        hash_result = hashlib.sha256(data).digest()
        
        # Check difficulty (leading zero bits)
        required_zeros = self.challenge.difficulty
        hash_int = int.from_bytes(hash_result, 'big')
        
        # Check if hash has required leading zeros
        return hash_int < (2 ** (256 - required_zeros))
    
    @classmethod
    def solve(cls, challenge: SybilChallenge, peer_pubkey: bytes) -> SybilProof:
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
            
            if solution % 10000 == 0:
                # Timeout after too many attempts
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
    def from_bytes(cls, data: bytes) -> SybilProof:
        """Deserialize proof."""
        challenge = SybilChallenge.from_bytes(data[:41])
        solution = int.from_bytes(data[41:49], 'big')
        peer_pubkey = data[49:]
        return cls(challenge=challenge, solution=solution, peer_pubkey=peer_pubkey)


class SybilResistantHandshake:
    """Sybil-resistant handshake protocol."""
    
    def __init__(self, difficulty: int = 8):
        self.difficulty = difficulty
    
    def create_challenge(self) -> SybilChallenge:
        """Create handshake challenge."""
        return SybilChallenge.create(difficulty=self.difficulty)
    
    def verify_proof(self, proof: SybilProof) -> bool:
        """Verify handshake proof."""
        # Check timestamp (not too old)
        age = time.time() - proof.challenge.timestamp
        if age > 300:  # 5 minutes max
            return False
        
        # Verify PoW
        return proof.verify()
    
    def solve_challenge(self, challenge: SybilChallenge, peer_pubkey: bytes) -> SybilProof:
        """Solve handshake challenge."""
        return SybilProof.solve(challenge, peer_pubkey)
