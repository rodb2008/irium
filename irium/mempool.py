"""Improved mempool management for Irium blockchain."""

from __future__ import annotations
import os
import json
import time
from typing import Dict, List, Optional, Set
from dataclasses import dataclass, field

from .tx import Transaction


@dataclass
class MempoolTransaction:
    """Transaction in mempool with metadata."""
    tx: Transaction
    tx_hex: str
    txid: str
    size: int
    fee: int
    fee_per_byte: float
    timestamp: float = field(default_factory=time.time)
    
    def to_dict(self) -> dict:
        """Convert to dictionary."""
        return {
            'txid': self.txid,
            'hex': self.tx_hex,
            'size': self.size,
            'fee': self.fee,
            'fee_per_byte': self.fee_per_byte,
            'timestamp': self.timestamp
        }


class Mempool:
    """Transaction mempool with validation and fee prioritization."""
    
    def __init__(self, data_dir: str = "~/.irium/mempool"):
        self.data_dir = os.path.expanduser(data_dir)
        self.pending_file = os.path.join(self.data_dir, "pending.json")
        
        # In-memory pool
        self.transactions: Dict[str, MempoolTransaction] = {}
        self.spent_outputs: Set[tuple] = set()  # (txid, index)
        
        # Configuration
        self.max_size = 1000  # Max transactions in mempool
        self.min_fee_per_byte = 1  # Minimum 1 satoshi per byte
        self.max_tx_age = 86400  # 24 hours
        
        # Load existing mempool
        self._load()
    
    def _load(self):
        """Load mempool from disk."""
        os.makedirs(self.data_dir, exist_ok=True)
        
        if not os.path.exists(self.pending_file):
            return
        
        try:
            with open(self.pending_file, 'r') as f:
                data = json.load(f)
            
            for tx_data in data:
                # Reconstruct MempoolTransaction
                tx_hex = tx_data['hex']
                tx_bytes = bytes.fromhex(tx_hex)
                
                # Calculate txid (simplified - would need proper deserialization)
                txid = tx_data.get('txid', tx_hex[:64])
                
                mempool_tx = MempoolTransaction(
                    tx=None,  # Would need to deserialize
                    tx_hex=tx_hex,
                    txid=txid,
                    size=tx_data.get('size', len(tx_bytes)),
                    fee=tx_data.get('fee', 0),
                    fee_per_byte=tx_data.get('fee_per_byte', 0),
                    timestamp=tx_data.get('timestamp', time.time())
                )
                
                self.transactions[txid] = mempool_tx
        
        except Exception as e:
            print(f"Error loading mempool: {e}")
    
    def _save(self):
        """Save mempool to disk."""
        try:
            data = [tx.to_dict() for tx in self.transactions.values()]
            
            with open(self.pending_file, 'w') as f:
                json.dump(data, f, indent=2)
        
        except Exception as e:
            print(f"Error saving mempool: {e}")
    
    def add_transaction(
        self,
        tx_hex: str,
        fee: int = 10000,
        validate: bool = True
    ) -> tuple[bool, str]:
        """
        Add transaction to mempool.
        
        Returns:
            (success, message)
        """
        try:
            tx_bytes = bytes.fromhex(tx_hex)
            size = len(tx_bytes)
            
            # Check size
            if size > 100000:  # 100KB max
                return False, "Transaction too large"
            
            # Calculate fee per byte
            fee_per_byte = fee / size if size > 0 else 0
            
            # Check minimum fee
            if fee_per_byte < self.min_fee_per_byte:
                return False, f"Fee too low (min: {self.min_fee_per_byte} sat/byte)"
            
            # Check mempool size
            if len(self.transactions) >= self.max_size:
                # Remove lowest fee transaction if this one has higher fee
                min_fee_tx = min(self.transactions.values(), key=lambda x: x.fee_per_byte)
                if fee_per_byte <= min_fee_tx.fee_per_byte:
                    return False, "Mempool full, fee too low"
                # Remove lowest fee tx
                del self.transactions[min_fee_tx.txid]
            
            # Generate txid (simplified)
            import hashlib
            txid = hashlib.sha256(tx_bytes).hexdigest()
            
            # Check if already in mempool
            if txid in self.transactions:
                return False, "Transaction already in mempool"
            
            # Create mempool transaction
            mempool_tx = MempoolTransaction(
                tx=None,
                tx_hex=tx_hex,
                txid=txid,
                size=size,
                fee=fee,
                fee_per_byte=fee_per_byte
            )
            
            # Add to mempool
            self.transactions[txid] = mempool_tx
            
            # Save to disk
            self._save()
            
            return True, f"Transaction added to mempool (fee: {fee_per_byte:.2f} sat/byte)"
        
        except Exception as e:
            return False, f"Error: {str(e)}"
    
    def remove_transaction(self, txid: str) -> bool:
        """Remove transaction from mempool."""
        if txid in self.transactions:
            del self.transactions[txid]
            self._save()
            return True
        return False
    
    def get_transactions(
        self,
        count: Optional[int] = None,
        min_fee: Optional[float] = None
    ) -> List[MempoolTransaction]:
        """
        Get transactions from mempool, sorted by fee (highest first).
        
        Args:
            count: Max number of transactions to return
            min_fee: Minimum fee per byte
        """
        txs = list(self.transactions.values())
        
        # Filter by min fee
        if min_fee is not None:
            txs = [tx for tx in txs if tx.fee_per_byte >= min_fee]
        
        # Sort by fee per byte (highest first)
        txs.sort(key=lambda x: x.fee_per_byte, reverse=True)
        
        # Limit count
        if count is not None:
            txs = txs[:count]
        
        return txs
    
    def cleanup_old_transactions(self):
        """Remove transactions older than max_tx_age."""
        now = time.time()
        to_remove = []
        
        for txid, tx in self.transactions.items():
            if now - tx.timestamp > self.max_tx_age:
                to_remove.append(txid)
        
        for txid in to_remove:
            del self.transactions[txid]
        
        if to_remove:
            self._save()
            print(f"Removed {len(to_remove)} old transactions from mempool")
    
    def clear(self):
        """Clear all transactions from mempool."""
        self.transactions.clear()
        self.spent_outputs.clear()
        self._save()
    
    def get_stats(self) -> dict:
        """Get mempool statistics."""
        if not self.transactions:
            return {
                'count': 0,
                'total_size': 0,
                'total_fees': 0,
                'avg_fee_per_byte': 0,
                'min_fee_per_byte': 0,
                'max_fee_per_byte': 0
            }
        
        txs = list(self.transactions.values())
        total_size = sum(tx.size for tx in txs)
        total_fees = sum(tx.fee for tx in txs)
        fee_per_bytes = [tx.fee_per_byte for tx in txs]
        
        return {
            'count': len(txs),
            'total_size': total_size,
            'total_fees': total_fees,
            'avg_fee_per_byte': sum(fee_per_bytes) / len(fee_per_bytes),
            'min_fee_per_byte': min(fee_per_bytes),
            'max_fee_per_byte': max(fee_per_bytes)
        }
