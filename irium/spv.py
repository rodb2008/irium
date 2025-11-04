"""Simple SPV header verification utilities.

Validates a sequence of block headers against checkpoints (anchors) and PoW targets.
This is designed for light clients that do not process full blocks.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Iterable, List, Optional, Sequence

from .block import BlockHeader
from .pow import Target, header_hash
from .constants import DIFFICULTY_RETARGET_INTERVAL, BLOCK_TARGET_INTERVAL


@dataclass(frozen=True)
class Anchor:
    height: int
    block_hash: bytes  # big-endian 32 bytes


class SpvVerifier:
    def __init__(self, pow_limit: Target, checkpoints: Sequence[Anchor] | None = None) -> None:
        self.pow_limit = pow_limit
        self.checkpoints = sorted(checkpoints or (), key=lambda a: a.height)

    def verify_chain(self, headers: Sequence[BlockHeader]) -> bool:
        if not headers:
            return False
        # Check anchors
        for anchor in self.checkpoints:
            if anchor.height < 0 or anchor.height >= len(headers):
                continue
            if headers[anchor.height].hash() != anchor.block_hash:
                return False
        # Check continuity and PoW
        for i, hdr in enumerate(headers):
            if i == 0:
                # Expect genesis prev_hash to be zero
                if hdr.prev_hash not in (b"\x00" * 32, bytes.fromhex("0000000040e3eb5ed9db5cc8df56dd6db9c6f3009ca7e9114fb52400e0136fb6")):
                    return False
            else:
                if hdr.prev_hash != headers[i - 1].hash():
                    return False
            # Check target bits transitions on retarget boundaries
            if i == 0:
                continue
            if i % DIFFICULTY_RETARGET_INTERVAL == 0:
                # At boundaries, clients should verify bits reflect expected retarget rules.
                # Light clients cannot recompute precisely without timestamps; ensure bits do not exceed pow_limit.
                if Target(hdr.bits).to_target() > self.pow_limit.to_target():
                    return False
            # Verify PoW meets declared target
            if int.from_bytes(hdr.hash(), "big") > Target(hdr.bits).to_target():
                return False
        return True


class NiPoPoW:
    """Non-Interactive Proofs of Proof-of-Work for super-light clients."""
    
    @staticmethod
    def is_superblock(header: BlockHeader, level: int) -> bool:
        """Check if header is a superblock at given level."""
        block_hash = header.hash()
        hash_int = int.from_bytes(block_hash[::-1], 'big')
        # Superblock has extra leading zeros
        return hash_int < (2 ** (256 - level))
    
    @staticmethod
    def filter_superblocks(headers: list[BlockHeader], level: int) -> list[BlockHeader]:
        """Filter headers to only superblocks at given level."""
        return [h for h in headers if NiPoPoW.is_superblock(h, level)]
    
    @staticmethod
    def verify_nipopow(
        superblocks: list[BlockHeader],
        level: int,
        claimed_work: int
    ) -> bool:
        """Verify a NiPoPoW proves sufficient work."""
        # Calculate work in superblocks
        work = len(superblocks) * (2 ** level)
        return work >= claimed_work
