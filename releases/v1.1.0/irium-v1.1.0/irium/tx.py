"""Transaction primitives for the Irium blockchain."""

from __future__ import annotations

from dataclasses import dataclass
from typing import List, Optional

from .constants import MAX_MONEY
from .pow import sha256d


@dataclass(frozen=True)
class TxInput:
    prev_txid: bytes
    prev_index: int
    script_sig: bytes
    sequence: int = 0xFFFFFFFF

    def serialize(self) -> bytes:
        if len(self.prev_txid) != 32:
            raise ValueError("prev_txid must be 32 bytes")
        if not (0 <= self.prev_index <= 0xFFFFFFFF):
            raise ValueError("prev_index out of range")
        if not (0 <= self.sequence <= 0xFFFFFFFF):
            raise ValueError("sequence out of range")
        if len(self.script_sig) > 0xFF:
            raise ValueError("script_sig too large")
        return (
            len(self.prev_txid).to_bytes(1, "big")
            + self.prev_txid
            + self.prev_index.to_bytes(4, "little")
            + len(self.script_sig).to_bytes(1, "big")
            + self.script_sig
            + self.sequence.to_bytes(4, "little")
        )


@dataclass(frozen=True)
class TxOutput:
    value: int
    script_pubkey: bytes

    def serialize(self) -> bytes:
        if not (0 <= self.value <= MAX_MONEY):
            raise ValueError("Output value out of range")
        if len(self.script_pubkey) > 0xFF:
            raise ValueError("script_pubkey too large")
        return (
            self.value.to_bytes(8, "little", signed=False)
            + len(self.script_pubkey).to_bytes(1, "big")
            + self.script_pubkey
        )


@dataclass(frozen=True)
class Transaction:
    version: int
    inputs: List[TxInput]
    outputs: List[TxOutput]
    locktime: int = 0

    def serialize(self) -> bytes:
        if not (0 <= self.version <= 0xFFFFFFFF):
            raise ValueError("version out of range")
        if not (0 <= self.locktime <= 0xFFFFFFFF):
            raise ValueError("locktime out of range")
        if len(self.inputs) > 0xFF:
            raise ValueError("Too many inputs for compact encoding")
        if len(self.outputs) > 0xFF:
            raise ValueError("Too many outputs for compact encoding")
        result = self.version.to_bytes(4, "little")
        result += len(self.inputs).to_bytes(1, "big")
        for txin in self.inputs:
            result += txin.serialize()
        result += len(self.outputs).to_bytes(1, "big")
        for txout in self.outputs:
            result += txout.serialize()
        result += self.locktime.to_bytes(4, "little")
        return result

    def txid(self) -> bytes:
        return sha256d(self.serialize())[::-1]

    def weight(self) -> int:
        return len(self.serialize()) * 4

    def fee(self, input_sum: int) -> int:
        output_sum = sum(o.value for o in self.outputs)
        return input_sum - output_sum


def cltv_script(lock_height: int, pubkey_hash: bytes) -> bytes:
    """Create a simple CLTV script locking coins until `lock_height`."""
    if len(pubkey_hash) != 20:
        raise ValueError("Expected 20-byte pubkey hash")
    if lock_height < 0:
        raise ValueError("Lock height must be non-negative")
    lock_bytes = _encode_locktime(lock_height)
    return (
        lock_bytes
        + b"\xb1"  # OP_CHECKLOCKTIMEVERIFY
        + b"\x75"  # OP_DROP
        + b"\x76\xa9"  # OP_DUP OP_HASH160
        + b"\x14" + pubkey_hash
        + b"\x88\xac"  # OP_EQUALVERIFY OP_CHECKSIG
    )


def _encode_locktime(value: int) -> bytes:
    """Encode locktime as minimal push data."""
    result = value.to_bytes((value.bit_length() + 7) // 8 or 1, "little")
    if result[-1] & 0x80:
        result += b"\x00"
    length = len(result)
    if length < 0x4c:
        return bytes([length]) + result
    raise ValueError("Locktime too large for simple encoding")
