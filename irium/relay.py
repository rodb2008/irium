"""Per-transaction relay rewards for Irium."""

from __future__ import annotations
from dataclasses import dataclass
from typing import Optional
import hashlib


@dataclass
class RelayCommitment:
    """Commitment to relay a transaction."""
    
    tx_hash: bytes  # Hash of transaction being relayed
    relay_pubkey: bytes  # Public key of relay node
    timestamp: int  # When relay occurred
    signature: Optional[bytes] = None  # Signature proving relay
    
    def compute_hash(self) -> bytes:
        """Compute hash of relay commitment."""
        data = self.tx_hash + self.relay_pubkey + self.timestamp.to_bytes(8, 'big')
        return hashlib.sha256(data).digest()
    
    def to_bytes(self) -> bytes:
        """Serialize to bytes."""
        data = (
            self.tx_hash +
            self.relay_pubkey +
            self.timestamp.to_bytes(8, 'big')
        )
        if self.signature:
            data += self.signature
        return data
    
    @classmethod
    def from_bytes(cls, data: bytes) -> RelayCommitment:
        """Deserialize from bytes."""
        tx_hash = data[:32]
        relay_pubkey = data[32:65]  # 33 bytes for compressed pubkey
        timestamp = int.from_bytes(data[65:73], 'big')
        signature = data[73:] if len(data) > 73 else None
        
        return cls(
            tx_hash=tx_hash,
            relay_pubkey=relay_pubkey,
            timestamp=timestamp,
            signature=signature
        )


class RelayRewardCalculator:
    """Calculate relay rewards for transactions."""
    
    def __init__(self, base_relay_fee: int = 100):
        """
        Initialize calculator.
        
        Args:
            base_relay_fee: Base relay fee in satoshis (default 100 = 0.000001 IRM)
        """
        self.base_relay_fee = base_relay_fee
        self.max_relays_per_tx = 3  # Max relays that can claim rewards
    
    def calculate_relay_reward(
        self,
        tx_fee: int,
        num_relays: int,
        relay_position: int
    ) -> int:
        """
        Calculate reward for a relay node.
        
        Args:
            tx_fee: Total transaction fee in satoshis
            num_relays: Total number of relays for this tx
            relay_position: Position of this relay (0 = first, 1 = second, etc.)
        
        Returns:
            Relay reward in satoshis
        """
        # Only reward up to max_relays_per_tx relays
        if relay_position >= self.max_relays_per_tx:
            return 0
        
        # Relay reward is a portion of the transaction fee
        relay_portion = 0.1  # 10% of tx fee goes to relays
        total_relay_reward = int(tx_fee * relay_portion)
        
        # Split among relays (first relay gets more)
        # First relay: 50%, Second: 30%, Third: 20%
        weights = [0.5, 0.3, 0.2]
        
        if relay_position < len(weights):
            reward = int(total_relay_reward * weights[relay_position])
            return max(reward, self.base_relay_fee)
        
        return 0
    
    def calculate_miner_fee(self, tx_fee: int, num_relays: int) -> int:
        """
        Calculate miner's portion of fee after relay rewards.
        
        Args:
            tx_fee: Total transaction fee
            num_relays: Number of relays
        
        Returns:
            Miner's fee in satoshis
        """
        total_relay_reward = 0
        
        for i in range(min(num_relays, self.max_relays_per_tx)):
            total_relay_reward += self.calculate_relay_reward(tx_fee, num_relays, i)
        
        # Miner gets the rest
        return tx_fee - total_relay_reward


@dataclass
class RelayProof:
    """Proof that a node relayed a transaction."""
    
    commitment: RelayCommitment
    hop_count: int  # Number of hops from origin
    
    def is_valid(self, tx_hash: bytes) -> bool:
        """Validate relay proof."""
        # Check transaction hash matches
        if self.commitment.tx_hash != tx_hash:
            return False
        
        # Check hop count is reasonable
        if self.hop_count > 10:  # Max 10 hops
            return False
        
        # Verify signature if present
        if self.commitment.signature:
            # TODO: Verify signature with relay_pubkey
            pass
        
        return True


class RelayTracker:
    """Track relay nodes for transactions."""
    
    def __init__(self):
        self.relay_map: dict[bytes, list[RelayCommitment]] = {}
        self.calculator = RelayRewardCalculator()
    
    def add_relay(self, tx_hash: bytes, relay: RelayCommitment):
        """Add a relay for a transaction."""
        if tx_hash not in self.relay_map:
            self.relay_map[tx_hash] = []
        
        # Only track up to max relays
        if len(self.relay_map[tx_hash]) < self.calculator.max_relays_per_tx:
            self.relay_map[tx_hash].append(relay)
    
    def get_relays(self, tx_hash: bytes) -> list[RelayCommitment]:
        """Get all relays for a transaction."""
        return self.relay_map.get(tx_hash, [])
    
    def calculate_rewards(self, tx_hash: bytes, tx_fee: int) -> dict[bytes, int]:
        """
        Calculate relay rewards for a transaction.
        
        Returns:
            Dictionary mapping relay_pubkey -> reward amount
        """
        relays = self.get_relays(tx_hash)
        rewards = {}
        
        for i, relay in enumerate(relays):
            reward = self.calculator.calculate_relay_reward(tx_fee, len(relays), i)
            if reward > 0:
                rewards[relay.relay_pubkey] = reward
        
        return rewards
