"""Network-wide constants for Irium mainnet."""

from __future__ import annotations

import dataclasses
from typing import Final


@dataclasses.dataclass(frozen=True)
class SubsidySchedule:
    """Describes the block subsidy emission curve."""

    initial_reward: int
    mining_supply: int
    coinbase_maturity: int

    def block_reward(self, height: int) -> int:
        """Return the block subsidy for a specific block height."""
        if height < 0:
            raise ValueError("Block height cannot be negative")
        if self.initial_reward <= 0:
            return 0
        if height == 0:
            return 0

        full_reward_blocks, remainder = divmod(self.mining_supply, self.initial_reward)
        if height <= full_reward_blocks:
            return self.initial_reward
        if remainder and height == full_reward_blocks + 1:
            return remainder
        return 0


MAX_MONEY: Final[int] = 100_000_000 * 10**8  # satoshis equivalent
BLOCK_TARGET_INTERVAL: Final[int] = 600  # seconds
DIFFICULTY_RETARGET_INTERVAL: Final[int] = 2016  # blocks
POW_ALGORITHM: Final[str] = "sha256d"
GENESIS_TIMELOCKS: Final[tuple[int, ...]] = (3 * 365 * 24 * 60 * 60,)
GENESIS_BLOCK_HEIGHT: Final[int] = 0
GENESIS_TOTAL_VESTING: Final[int] = 3_500_000 * 10**8
GENESIS_PUBLIC_SUPPLY: Final[int] = 96_500_000 * 10**8
PUBKEY_ADDRESS_PREFIX: Final[int] = 0x39  # Base58 prefix for Irium P2PKH addresses
SUBSIDY_SCHEDULE: Final[SubsidySchedule] = SubsidySchedule(
    initial_reward=50 * 10**8,
    mining_supply=GENESIS_PUBLIC_SUPPLY,
    coinbase_maturity=100,
)
