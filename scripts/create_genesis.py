"""Create the Irium genesis block definition."""

from __future__ import annotations

import argparse
import json
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import List

from irium.block import Block, BlockHeader
from irium.constants import GENESIS_BLOCK_HEIGHT
from irium.pow import Target, sha256d
from irium.tx import Transaction, TxInput, TxOutput


@dataclass
class GenesisAllocation:
    amount: int
    script_pubkey: str


def create_genesis_block(timestamp: int, bits: int, nonce: int, allocations: List[GenesisAllocation]) -> Block:
    coinbase_script = b"Irium genesis block"
    coinbase_tx = Transaction(
        version=1,
        inputs=[TxInput(prev_txid=b"\x00" * 32, prev_index=0xFFFFFFFF, script_sig=coinbase_script)],
        outputs=[TxOutput(value=a.amount, script_pubkey=bytes.fromhex(a.script_pubkey)) for a in allocations],
        locktime=0,
    )

    header = BlockHeader(
        version=1,
        prev_hash=b"\x00" * 32,
        merkle_root=coinbase_tx.txid(),
        time=timestamp,
        bits=bits,
        nonce=nonce,
    )
    return Block(header=header, transactions=[coinbase_tx])


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--timestamp", type=int, required=True)
    parser.add_argument("--bits", type=lambda x: int(x, 16), required=True)
    parser.add_argument("--nonce", type=int, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--allocation", action="append", nargs=2, metavar=("amount", "script"), default=[])
    args = parser.parse_args()

    allocations = [GenesisAllocation(amount=int(amount), script_pubkey=script) for amount, script in args.allocation]
    genesis_block = create_genesis_block(args.timestamp, args.bits, args.nonce, allocations)
    data = {
        "height": GENESIS_BLOCK_HEIGHT,
        "header": {
            "version": genesis_block.header.version,
            "prev_hash": genesis_block.header.prev_hash.hex(),
            "merkle_root": genesis_block.header.merkle_root.hex(),
            "time": genesis_block.header.time,
            "bits": f"{genesis_block.header.bits:08x}",
            "nonce": genesis_block.header.nonce,
            "hash": genesis_block.header.hash().hex(),
        },
        "transactions": [tx.serialize().hex() for tx in genesis_block.transactions],
    }
    args.output.write_text(json.dumps(data, indent=2))


if __name__ == "__main__":
    main()
