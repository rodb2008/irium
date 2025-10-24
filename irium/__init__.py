"""Irium blockchain library."""

__version__ = "1.2.0"

# Core blockchain
from .block import Block, BlockHeader
from .chain import ChainParams, ChainState
from .tx import Transaction, TxInput, TxOutput
from .wallet import Wallet, KeyPair
from .pow import Target

# Network and P2P
from .network import PeerDirectory, SeedlistManager
from .protocol import Message, MessageType
from .p2p import P2PNode

# Advanced features
from .mempool import Mempool, MempoolTransaction
from .relay import RelayCommitment, RelayRewardCalculator
from .uptime import UptimeProof, PeerReputation
from .sybil import SybilChallenge, SybilProof
from .anchors import AnchorManager, EclipseProtection
from .spv import SpvVerifier, NiPoPoW

__all__ = [
    # Core
    'Block', 'BlockHeader',
    'ChainParams', 'ChainState',
    'Transaction', 'TxInput', 'TxOutput',
    'Wallet', 'KeyPair',
    'Target',
    # Network
    'PeerDirectory', 'SeedlistManager',
    'Message', 'MessageType',
    'P2PNode',
    # Advanced
    'Mempool', 'MempoolTransaction',
    'RelayCommitment', 'RelayRewardCalculator',
    'UptimeProof', 'PeerReputation',
    'SybilChallenge', 'SybilProof',
    'AnchorManager', 'EclipseProtection',
    'SpvVerifier', 'NiPoPoW',
]
