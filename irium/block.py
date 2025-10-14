"""Block primitives for the Irium blockchain."""

from __future__ import annotations

import time
from dataclasses import dataclass, field
from typing import List

from .pow import Target, header_hash
from .tx import Transaction


@dataclass
class BlockHeader:
    version: int
    prev_hash: bytes
    merkle_root: bytes
    time: int
    bits: int
    nonce: int

    def serialize(self) -> bytes:
        return (
            self.version.to_bytes(4, "little")
            + self.prev_hash[::-1]
            + self.merkle_root[::-1]
            + self.time.to_bytes(4, "little")
            + self.bits.to_bytes(4, "little")
            + self.nonce.to_bytes(4, "little")
        )

    def hash(self) -> bytes:
        return header_hash([self.serialize()])[::-1]

    @property
    def target(self) -> Target:
        return Target(self.bits)


@dataclass
class Block:
    header: BlockHeader
    transactions: List[Transaction] = field(default_factory=list)

    def merkle_root(self) -> bytes:
        if not self.transactions:
            return bytes.fromhex(
                "0" * 64
            )
        leaves = [tx.txid()[::-1] for tx in self.transactions]
        while len(leaves) > 1:
            if len(leaves) % 2 == 1:
                leaves.append(leaves[-1])
            leaves = [header_hash([leaves[i], leaves[i + 1]]) for i in range(0, len(leaves), 2)]
        return leaves[0]

    def update_merkle_root(self) -> None:
        self.header.merkle_root = self.merkle_root()[::-1]

    def mine(self) -> None:
        self.update_merkle_root()
        target = self.header.target
        nonce = 0
        while True:
            self.header.nonce = nonce
            block_hash = self.header.hash()[::-1]
            if int.from_bytes(block_hash, "big") <= target.to_target():
                break
            nonce += 1

    @property
    def size(self) -> int:
        return len(self.header.serialize()) + sum(len(tx.serialize()) for tx in self.transactions)

    @property
    def weight(self) -> int:
        return self.size * 4
