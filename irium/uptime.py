"""Uptime proof system for peer reputation in Irium."""

from __future__ import annotations
import time
import hashlib
import hmac
from dataclasses import dataclass
from typing import Optional


@dataclass
class UptimeProof:
    """Proof of uptime for peer reputation."""
    
    peer_id: str  # Peer identifier
    timestamp: int  # Unix timestamp
    challenge: bytes  # Random challenge
    response: bytes  # HMAC response
    
    @classmethod
    def create_challenge(cls) -> bytes:
        """Create a random challenge."""
        import secrets
        return secrets.token_bytes(32)
    
    @classmethod
    def create_response(cls, challenge: bytes, peer_secret: bytes) -> bytes:
        """Create HMAC response to challenge."""
        return hmac.new(peer_secret, challenge, hashlib.sha256).digest()
    
    @classmethod
    def verify_response(cls, challenge: bytes, response: bytes, peer_secret: bytes) -> bool:
        """Verify HMAC response."""
        expected = hmac.new(peer_secret, challenge, hashlib.sha256).digest()
        return hmac.compare_digest(response, expected)


@dataclass
class PeerReputation:
    """Track peer reputation based on uptime and behavior."""
    
    peer_id: str
    score: int = 100  # Start at 100
    successful_connections: int = 0
    failed_connections: int = 0
    blocks_received: int = 0
    invalid_blocks: int = 0
    last_seen: float = 0.0
    uptime_proofs: int = 0
    
    def update_score(self):
        """Recalculate reputation score."""
        # Base score
        score = 100
        
        # Reward successful connections
        score += self.successful_connections * 2
        
        # Penalize failed connections
        score -= self.failed_connections * 5
        
        # Reward valid blocks
        score += self.blocks_received * 10
        
        # Heavily penalize invalid blocks
        score -= self.invalid_blocks * 50
        
        # Reward uptime proofs
        score += self.uptime_proofs * 5
        
        # Penalize old peers (not seen recently)
        time_since_seen = time.time() - self.last_seen
        if time_since_seen > 3600:  # 1 hour
            score -= int(time_since_seen / 3600) * 2
        
        # Keep score in reasonable range
        self.score = max(0, min(1000, score))
    
    def record_connection_success(self):
        """Record successful connection."""
        self.successful_connections += 1
        self.last_seen = time.time()
        self.update_score()
    
    def record_connection_failure(self):
        """Record failed connection."""
        self.failed_connections += 1
        self.update_score()
    
    def record_valid_block(self):
        """Record receiving a valid block."""
        self.blocks_received += 1
        self.last_seen = time.time()
        self.update_score()
    
    def record_invalid_block(self):
        """Record receiving an invalid block."""
        self.invalid_blocks += 1
        self.update_score()
    
    def record_uptime_proof(self):
        """Record successful uptime proof."""
        self.uptime_proofs += 1
        self.last_seen = time.time()
        self.update_score()
    
    def is_trusted(self) -> bool:
        """Check if peer is trusted (score > 80)."""
        return self.score > 80
    
    def is_banned(self) -> bool:
        """Check if peer should be banned (score < 20)."""
        return self.score < 20
    
    def to_dict(self) -> dict:
        """Convert to dictionary."""
        return {
            'peer_id': self.peer_id,
            'score': self.score,
            'successful_connections': self.successful_connections,
            'failed_connections': self.failed_connections,
            'blocks_received': self.blocks_received,
            'invalid_blocks': self.invalid_blocks,
            'uptime_proofs': self.uptime_proofs,
            'last_seen': self.last_seen,
            'is_trusted': self.is_trusted(),
            'is_banned': self.is_banned()
        }


class ReputationManager:
    """Manage peer reputation across the network."""
    
    def __init__(self, data_file: str = "~/.irium/peer_reputation.json"):
        self.data_file = os.path.expanduser(data_file)
        self.reputations: dict[str, PeerReputation] = {}
        self._load()
    
    def _load(self):
        """Load reputation data from disk."""
        if not os.path.exists(self.data_file):
            return
        
        try:
            import json
            with open(self.data_file, 'r') as f:
                data = json.load(f)
            
            for peer_id, rep_data in data.items():
                rep = PeerReputation(
                    peer_id=peer_id,
                    score=rep_data.get('score', 100),
                    successful_connections=rep_data.get('successful_connections', 0),
                    failed_connections=rep_data.get('failed_connections', 0),
                    blocks_received=rep_data.get('blocks_received', 0),
                    invalid_blocks=rep_data.get('invalid_blocks', 0),
                    uptime_proofs=rep_data.get('uptime_proofs', 0),
                    last_seen=rep_data.get('last_seen', 0.0)
                )
                rep.update_score()
                self.reputations[peer_id] = rep
        
        except Exception as e:
            print(f"Error loading reputation data: {e}")
    
    def _save(self):
        """Save reputation data to disk."""
        try:
            import json
            import os
            
            os.makedirs(os.path.dirname(self.data_file), exist_ok=True)
            
            data = {
                peer_id: rep.to_dict()
                for peer_id, rep in self.reputations.items()
            }
            
            with open(self.data_file, 'w') as f:
                json.dump(data, f, indent=2)
        
        except Exception as e:
            print(f"Error saving reputation data: {e}")
    
    def get_reputation(self, peer_id: str) -> PeerReputation:
        """Get or create reputation for peer."""
        if peer_id not in self.reputations:
            self.reputations[peer_id] = PeerReputation(peer_id=peer_id)
        return self.reputations[peer_id]
    
    def get_trusted_peers(self) -> list[PeerReputation]:
        """Get list of trusted peers."""
        return [rep for rep in self.reputations.values() if rep.is_trusted()]
    
    def get_banned_peers(self) -> list[PeerReputation]:
        """Get list of banned peers."""
        return [rep for rep in self.reputations.values() if rep.is_banned()]
    
    def cleanup_old_reputations(self, max_age: int = 604800):
        """Remove reputation data for peers not seen in max_age seconds (default 7 days)."""
        now = time.time()
        to_remove = []
        
        for peer_id, rep in self.reputations.items():
            if now - rep.last_seen > max_age:
                to_remove.append(peer_id)
        
        for peer_id in to_remove:
            del self.reputations[peer_id]
        
        if to_remove:
            self._save()


# === AutoPatch: Peer uptime tracking ===
import json, os, datetime
SEED_FILE = os.path.expanduser("~/.irium/seeds.txt")

def load_peer_uptime():
    if not os.path.exists(SEED_FILE):
        return {}
    try:
        with open(SEED_FILE) as f:
            return json.load(f)
    except Exception:
        return {}

def save_peer_uptime(data):
    os.makedirs(os.path.dirname(SEED_FILE), exist_ok=True)
    with open(SEED_FILE, "w") as f:
        json.dump(data, f, indent=2)

def record_peer_uptime(ip):
    data = load_peer_uptime()
    now = datetime.datetime.utcnow().timestamp()
    peer = data.get(ip, {"first_seen": now, "last_seen": now})
    peer["last_seen"] = now
    data[ip] = peer
    save_peer_uptime(data)

def prune_peers():
    data = load_peer_uptime()
    now = datetime.datetime.utcnow().timestamp()
    keep = {}
    for ip, meta in data.items():
        age_days = (now - meta["first_seen"]) / 86400
        inactive = (now - meta["last_seen"]) / 86400
        if age_days >= 7:
            meta["trusted"] = True
        if inactive < 1:
            keep[ip] = meta
    save_peer_uptime(keep)
# === End AutoPatch ===
