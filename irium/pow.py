"""Proof-of-Work utilities for Irium."""

from __future__ import annotations

import hashlib
from dataclasses import dataclass
from typing import Iterable


@dataclass(frozen=True)
class Target:
    """Compact representation of the PoW target."""

    bits: int

    def to_target(self) -> int:
        exponent = self.bits >> 24
        mantissa = self.bits & 0xFFFFFF
        if exponent <= 3:
            value = mantissa >> (8 * (3 - exponent))
        else:
            value = mantissa << (8 * (exponent - 3))
        return value

    def difficulty(self) -> float:
        base_target = Target(0x1d00ffff).to_target()
        return base_target / self.to_target()

    @classmethod
    def from_target(cls, value: int) -> "Target":
        exponent = (value.bit_length() + 7) // 8
        if exponent <= 3:
            mantissa = value << (8 * (3 - exponent))
        else:
            mantissa = value >> (8 * (exponent - 3))
        if mantissa & 0x800000:
            mantissa >>= 8
            exponent += 1
        bits = (exponent << 24) | (mantissa & 0xFFFFFF)
        return cls(bits)


def sha256d(data: bytes) -> bytes:
    return hashlib.sha256(hashlib.sha256(data).digest()).digest()


def header_hash(parts: Iterable[bytes]) -> bytes:
    return sha256d(b"".join(parts))


def meets_target(hash_bytes: bytes, target: Target) -> bool:
    return int.from_bytes(hash_bytes, byteorder="big") <= target.to_target()
