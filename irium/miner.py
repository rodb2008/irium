"""Mining loop utilities for the Irium mainnet."""

from __future__ import annotations

import os
import time
from dataclasses import dataclass
from typing import Iterable, List, Optional, Sequence

from .block import Block, BlockHeader
from .chain import ChainState
from .constants import BLOCK_TARGET_INTERVAL, SUBSIDY_SCHEDULE
from .tx import Transaction, TxInput, TxOutput
from .wallet import Wallet, address_to_script

_MAX_BLOCK_WEIGHT = 4_000_000  # Segwit-style weight limit analogue


@dataclass(frozen=True)
class TxCandidate:
    """Transaction plus accounting metadata for block assembly."""

    transaction: Transaction
    fee: int
    weight: int

    @property
    def fee_rate(self) -> float:
        if self.weight <= 0:
            raise ValueError("Transaction weight must be positive")
        return self.fee / self.weight


@dataclass(frozen=True)
class RelayCommitment:
    """Describe a fee-sharing payout for a relay peer."""

    address: str
    amount: int
    memo: str | None = None

    def build_outputs(self) -> List[TxOutput]:
        script = address_to_script(self.address)
        outputs = [TxOutput(value=self.amount, script_pubkey=script)]
        if self.memo:
            memo_bytes = self.memo.encode("utf8")
            if len(memo_bytes) > 64:
                raise ValueError("Relay memo exceeds 64 bytes")
            outputs.append(_commitment_output(b"relay:" + memo_bytes))
        return outputs


@dataclass
class MiningStats:
    """Expose aggregate information about a mined block."""

    height: int
    total_fees: int
    attempts: int
    duration: float


class Miner:
    """Assemble block templates and iterate proof-of-work solutions."""

    def __init__(
        self,
        chain_state: ChainState,
        payout_address: str,
        wallet: Optional[Wallet] = None,
        *,
        max_block_weight: int = _MAX_BLOCK_WEIGHT,
    ) -> None:
        self.chain_state = chain_state
        self.payout_address = payout_address
        self.wallet = wallet
        self.max_block_weight = max_block_weight
        self._extra_nonce = int.from_bytes(os.urandom(4), "big")
        self._last_coinbase_reward = 0

    def mine_block(
        self,
        candidates: Iterable[TxCandidate],
        *,
        relay_commitments: Sequence[RelayCommitment] | None = None,
        timestamp: Optional[int] = None,
        max_attempts: Optional[int] = None,
    ) -> tuple[Optional[Block], MiningStats]:
        """Attempt to mine a new block from the provided mempool candidates."""

        start = time.time()
        relay_commitments = tuple(relay_commitments or ())
        selected, total_fees = self._select_transactions(candidates)
        height = self.chain_state.height
        block = self._create_candidate_block(
            selected,
            total_fees,
            relay_commitments,
            timestamp=timestamp,
            height=height,
        )

        attempts = 0
        solved = False
        while True:
            target_attempts = None if max_attempts is None else max_attempts - attempts
            solved, made = self._solve_block(block, max_attempts=target_attempts)
            attempts += made
            if solved or (max_attempts is not None and attempts >= max_attempts):
                break
            self._extra_nonce = (self._extra_nonce + 1) & 0xFFFFFFFF
            coinbase, miner_reward = self._coinbase_transaction(
                height,
                total_fees,
                relay_commitments,
                extra_nonce=self._extra_nonce,
            )
            self._last_coinbase_reward = miner_reward
            block.transactions[0] = coinbase
            block.update_merkle_root()
            block.header.time = self._current_time(block.header.time)
            block.header.nonce = 0

        if solved:
            self._register_coinbase(block)
            duration = time.time() - start
            stats = MiningStats(height=height, total_fees=total_fees, attempts=attempts, duration=duration)
            return block, stats
        duration = time.time() - start
        stats = MiningStats(height=height, total_fees=total_fees, attempts=attempts, duration=duration)
        return None, stats

    def _select_transactions(self, candidates: Iterable[TxCandidate]) -> tuple[List[Transaction], int]:
        remaining_weight = self.max_block_weight
        selected: List[Transaction] = []
        total_fees = 0
        ordered = sorted(candidates, key=lambda c: c.fee_rate, reverse=True)
        for candidate in ordered:
            if candidate.weight > remaining_weight:
                continue
            selected.append(candidate.transaction)
            remaining_weight -= candidate.weight
            total_fees += candidate.fee
        return selected, total_fees

    def _create_candidate_block(
        self,
        transactions: Sequence[Transaction],
        total_fees: int,
        relay_commitments: Sequence[RelayCommitment],
        *,
        timestamp: Optional[int],
        height: int,
    ) -> Block:
        prev_block = self.chain_state.chain[-1]
        header = BlockHeader(
            version=1,
            prev_hash=prev_block.header.hash(),
            merkle_root=b"\x00" * 32,
            time=self._current_time(prev_block.header.time) if timestamp is None else max(timestamp, prev_block.header.time + 1),
            bits=self.chain_state.target_for_height(height).bits,
            nonce=0,
        )
        coinbase, miner_reward = self._coinbase_transaction(height, total_fees, relay_commitments, extra_nonce=self._extra_nonce)
        self._last_coinbase_reward = miner_reward
        block_transactions = [coinbase, *transactions]
        block = Block(header=header, transactions=list(block_transactions))
        block.update_merkle_root()
        return block

    def _coinbase_transaction(
        self,
        height: int,
        total_fees: int,
        relay_commitments: Sequence[RelayCommitment],
        *,
        extra_nonce: int,
    ) -> tuple[Transaction, int]:
        script_sig = _coinbase_script(height, extra_nonce)
        subsidy = SUBSIDY_SCHEDULE.block_reward(height)
        distributed = 0
        commitment_outputs: List[TxOutput] = []
        for commitment in relay_commitments:
            distributed += commitment.amount
            if distributed > total_fees:
                raise ValueError("Relay commitments exceed total fees")
            commitment_outputs.extend(commitment.build_outputs())
        miner_reward = subsidy + (total_fees - distributed)
        if miner_reward < 0:
            raise ValueError("Negative miner reward computed")
        outputs: List[TxOutput] = [
            TxOutput(value=miner_reward, script_pubkey=address_to_script(self.payout_address)),
            *commitment_outputs,
        ]
        tx = Transaction(
            version=1,
            inputs=[
                TxInput(
                    prev_txid=b"\x00" * 32,
                    prev_index=0xFFFFFFFF,
                    script_sig=script_sig,
                    sequence=0,
                )
            ],
            outputs=outputs,
            locktime=0,
        )
        return tx, miner_reward

    def _solve_block(self, block: Block, *, max_attempts: Optional[int]) -> tuple[bool, int]:
        target = block.header.target
        attempts = 0
        while True:
            block_hash = block.header.hash()[::-1]
            attempts += 1
            if int.from_bytes(block_hash, "big") <= target.to_target():
                return True, attempts
            block.header.nonce = (block.header.nonce + 1) & 0xFFFFFFFF
            if block.header.nonce == 0:
                return False, attempts
            if max_attempts is not None and attempts >= max_attempts:
                return False, attempts

    def _register_coinbase(self, block: Block) -> None:
        if self.wallet is None:
            return
        coinbase = block.transactions[0]
        self.wallet.register_utxo(coinbase.txid(), 0, self._last_coinbase_reward, self.payout_address)

    def _current_time(self, previous_time: int) -> int:
        now = int(time.time())
        if now <= previous_time:
            return previous_time + 1
        if now - previous_time > BLOCK_TARGET_INTERVAL:
            return previous_time + BLOCK_TARGET_INTERVAL
        return now


def _coinbase_script(height: int, extra_nonce: int) -> bytes:
    height_bytes = _encode_compact(height)
    nonce_bytes = _encode_compact(extra_nonce)
    return height_bytes + nonce_bytes + _encode_push(b"Irium miner")


def _commitment_output(data: bytes) -> TxOutput:
    if len(data) > 75:
        raise ValueError("Commitment payload too large")
    script = b"\x6a" + len(data).to_bytes(1, "big") + data
    return TxOutput(value=0, script_pubkey=script)


def _encode_compact(value: int) -> bytes:
    if value < 0:
        raise ValueError("Value must be non-negative")
    raw = value.to_bytes((value.bit_length() + 7) // 8 or 1, "little")
    if raw[-1] & 0x80:
        raw += b"\x00"
    return len(raw).to_bytes(1, "big") + raw


def _encode_push(data: bytes) -> bytes:
    if len(data) >= 0x4C:
        raise ValueError("Pushdata too large for coinbase message")
    return len(data).to_bytes(1, "big") + data


__all__ = [
    "Miner",
    "TxCandidate",
    "RelayCommitment",
    "MiningStats",
]
