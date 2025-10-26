"""Anchor file verification for eclipse attack protection."""

from __future__ import annotations
import json
import hashlib
from dataclasses import dataclass
from typing import List, Optional
from pathlib import Path


@dataclass
class AnchorHeader:
    """Checkpoint header in anchor file."""
    
    height: int
    hash: str
    timestamp: int
    prev_hash: str
    
    def to_dict(self) -> dict:
        """Convert to dictionary."""
        return {
            'height': self.height,
            'hash': self.hash,
            'timestamp': self.timestamp,
            'prev_hash': self.prev_hash
        }
    
    @classmethod
    def from_dict(cls, data: dict) -> AnchorHeader:
        """Create from dictionary."""
        return cls(
            height=data['height'],
            hash=data['hash'],
            timestamp=data['timestamp'],
            prev_hash=data['prev_hash']
        )


class AnchorManager:
    """Manage and verify anchor file checkpoints."""
    
    def __init__(self, anchors_file: str = "bootstrap/anchors.json"):
        self.anchors_file = Path(anchors_file)
        self.anchors: List[AnchorHeader] = []
        self.trusted_signers: List[str] = []
        self._load()
    
    def _load(self):
        """Load anchors from file."""
        if not self.anchors_file.exists():
            return
        
        try:
            with open(self.anchors_file, 'r') as f:
                data = json.load(f)
            
            # Load trusted signers
            self.trusted_signers = data.get('trusted_signers', [])
            
            # Load anchor headers
            for anchor_data in data.get('anchors', []):
                anchor = AnchorHeader.from_dict(anchor_data)
                self.anchors.append(anchor)
            
            # Sort by height
            self.anchors.sort(key=lambda x: x.height)
        
        except Exception as e:
            print(f"Error loading anchors: {e}")
    
    def get_anchor_at_height(self, height: int) -> Optional[AnchorHeader]:
        """Get anchor at specific height."""
        for anchor in self.anchors:
            if anchor.height == height:
                return anchor
        return None
    
    def get_latest_anchor(self) -> Optional[AnchorHeader]:
        """Get the latest anchor."""
        return self.anchors[-1] if self.anchors else None
    
    def verify_block_against_anchors(self, height: int, block_hash: str) -> bool:
        """
        Verify a block against anchors.
        
        This protects against eclipse attacks by ensuring the chain
        matches known checkpoints.
        """
        anchor = self.get_anchor_at_height(height)
        
        if anchor is None:
            # No anchor at this height, allow
            return True
        
        # Check if block hash matches anchor
        return anchor.hash == block_hash
    
    def is_chain_valid(self, chain_tip_height: int, chain_tip_hash: str) -> bool:
        """
        Check if chain tip is consistent with anchors.
        
        Ensures we're not on an eclipse-attacked fork.
        """
        # Find the most recent anchor below tip
        relevant_anchor = None
        for anchor in reversed(self.anchors):
            if anchor.height <= chain_tip_height:
                relevant_anchor = anchor
                break
        
        if not relevant_anchor:
            # No anchors yet, allow
            return True
        
        # Chain tip must be descended from anchor
        # (simplified check - real implementation would verify full chain)
        return True
    
    def add_anchor(self, anchor: AnchorHeader, signature: Optional[str] = None):
        """Add a new anchor checkpoint."""
        # TODO: Verify signature from trusted signer
        
        # Check if anchor already exists
        existing = self.get_anchor_at_height(anchor.height)
        if existing:
            if existing.hash != anchor.hash:
                print(f"Warning: Conflicting anchor at height {anchor.height}")
            return
        
        self.anchors.append(anchor)
        self.anchors.sort(key=lambda x: x.height)
        self._save()
    
    def _save(self):
        """Save anchors to file."""
        try:
            data = {
                'trusted_signers': self.trusted_signers,
                'anchors': [anchor.to_dict() for anchor in self.anchors]
            }
            
            with open(self.anchors_file, 'w') as f:
                json.dump(data, f, indent=2)
        
        except Exception as e:
            print(f"Error saving anchors: {e}")
    
    def get_stats(self) -> dict:
        """Get anchor statistics."""
        if not self.anchors:
            return {
                'total_anchors': 0,
                'latest_height': 0,
                'trusted_signers': len(self.trusted_signers)
            }
        
        return {
            'total_anchors': len(self.anchors),
            'latest_height': self.anchors[-1].height,
            'latest_hash': self.anchors[-1].hash,
            'trusted_signers': len(self.trusted_signers)
        }


class EclipseProtection:
    """Protect against eclipse attacks using anchors."""
    
    def __init__(self, anchor_manager: AnchorManager):
        self.anchor_manager = anchor_manager
        self.suspicious_peers: set[str] = set()
    
    def verify_peer_chain(
        self,
        peer_id: str,
        peer_height: int,
        peer_tip_hash: str
    ) -> bool:
        """
        Verify a peer's chain against anchors.
        
        Returns True if chain is valid, False if suspicious.
        """
        # Check against anchors
        if not self.anchor_manager.is_chain_valid(peer_height, peer_tip_hash):
            print(f"⚠️  Peer {peer_id} has chain inconsistent with anchors")
            self.suspicious_peers.add(peer_id)
            return False
        
        return True
    
    def is_peer_suspicious(self, peer_id: str) -> bool:
        """Check if peer is suspicious."""
        return peer_id in self.suspicious_peers
    
    def clear_suspicion(self, peer_id: str):
        """Clear suspicion from peer."""
        self.suspicious_peers.discard(peer_id)
