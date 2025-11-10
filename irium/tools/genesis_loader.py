"""Helpers for loading the canonical locked genesis data."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Tuple

from irium.block import Block, BlockHeader
from irium.tx import Transaction, TxInput, TxOutput


class _Buffer:
    """Tiny binary reader for the simplified transaction format."""

    def __init__(self, data: bytes) -> None:
        self._data = data
        self._offset = 0

    def read_uint8(self) -> int:
        value = self._data[self._offset]
        self._offset += 1
        return value

    def read_uint32(self) -> int:
        value = int.from_bytes(self._data[self._offset : self._offset + 4], "little")
        self._offset += 4
        return value

    def read_uint64(self) -> int:
        value = int.from_bytes(self._data[self._offset : self._offset + 8], "little", signed=False)
        self._offset += 8
        return value

    def read_bytes(self, length: int) -> bytes:
        chunk = self._data[self._offset : self._offset + length]
        if len(chunk) != length:
            raise ValueError("Unexpected end of transaction while reading bytes")
        self._offset += length
        return bytes(chunk)

    def read_compact_bytes(self) -> bytes:
        length = self.read_uint8()
        return self.read_bytes(length)

    def ensure_consumed(self) -> None:
        if self._offset != len(self._data):
            raise ValueError("Trailing data detected while decoding transaction")


def decode_transaction_hex(tx_hex: str) -> Transaction:
    """Decode a compact Transaction representation used for the locked genesis."""

    raw = bytes.fromhex(tx_hex)
    buf = _Buffer(raw)
    version = buf.read_uint32()

    input_count = buf.read_uint8()
    inputs: list[TxInput] = []
    for _ in range(input_count):
        prev_txid = buf.read_compact_bytes()
        prev_index = buf.read_uint32()
        script_sig = buf.read_compact_bytes()
        sequence = buf.read_uint32()
        inputs.append(
            TxInput(
                prev_txid=prev_txid,
                prev_index=prev_index,
                script_sig=script_sig,
                sequence=sequence,
            )
        )

    output_count = buf.read_uint8()
    outputs: list[TxOutput] = []
    for _ in range(output_count):
        value = buf.read_uint64()
        script_pubkey = buf.read_compact_bytes()
        outputs.append(TxOutput(value=value, script_pubkey=script_pubkey))

    locktime = buf.read_uint32()
    buf.ensure_consumed()

    return Transaction(version=version, inputs=inputs, outputs=outputs, locktime=locktime)


def load_locked_genesis(base_dir: Path | None = None) -> Tuple[Block, dict]:
    """
    Load the canonical genesis block from config/genesis-locked.json.

    Returns the constructed Block plus the parsed JSON payload for callers
    that still need direct header fields.
    """

    repo_root = base_dir or Path(__file__).resolve().parents[2]
    genesis_path = repo_root / "config" / "genesis-locked.json"
    data = json.loads(genesis_path.read_text())
    header = data["header"]
    transactions = [decode_transaction_hex(tx_hex) for tx_hex in data.get("transactions", [])]

    block = Block(
        header=BlockHeader(
            version=header["version"],
            prev_hash=bytes.fromhex(header["prev_hash"]),
            merkle_root=bytes.fromhex(header["merkle_root"]),
            time=header["time"],
            bits=int(header["bits"], 16),
            nonce=header["nonce"],
        ),
        transactions=transactions,
    )

    return block, data
