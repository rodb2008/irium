"""Peer networking utilities for Irium's zero-DNS topology."""

from __future__ import annotations

import json
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Dict, Iterable, Optional


SEEDLIST_PATH = Path("bootstrap/seedlist.txt")
RUNTIME_SEEDLIST_PATH = Path("bootstrap/seedlist.runtime")
PEER_DB_PATH = Path("state/peers.json")


def _normalize_multiaddr(addr: str) -> str:
    candidate = addr.strip()
    if not candidate:
        raise ValueError("Empty multiaddr")
    if not candidate.startswith("/"):
        raise ValueError("Multiaddr must start with '/'")
    return candidate


@dataclass
class PeerRecord:
    """Track observed peer metadata for auto-healing discovery."""

    multiaddr: str
    agent: Optional[str] = None
    last_seen: float = field(default_factory=time.time)
    first_seen: float = field(default_factory=time.time)

    def touch(self) -> None:
        self.last_seen = time.time()


class SeedlistManager:
    """Maintain a runtime seedlist that augments the signed release file."""

    def __init__(self, baseline: Path = SEEDLIST_PATH, runtime: Path = RUNTIME_SEEDLIST_PATH, limit: int = 512) -> None:
        self.baseline = baseline
        self.runtime = runtime
        self.limit = limit
        self.runtime.parent.mkdir(parents=True, exist_ok=True)
        if not self.runtime.exists():
            header = "# Auto-generated runtime seedlist. Do not edit manually.\n"
            self.runtime.write_text(header)

    def _load_runtime_entries(self) -> Iterable[str]:
        if not self.runtime.exists():
            return []
        entries = []
        for line in self.runtime.read_text().splitlines():
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            entries.append(line)
        return entries

    def write_runtime_entries(self, entries: Iterable[str]) -> None:
        """Persist the given iterable of multiaddrs as the runtime seedlist."""
        normalised = [ _normalize_multiaddr(addr) for addr in entries ]
        # De-duplicate while preserving order
        unique = list(dict.fromkeys(normalised))[: self.limit]
        timestamp = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        header = f"# Runtime seedlist refreshed {timestamp}\n"
        body = "\n".join(unique) + ("\n" if unique else "")
        self.runtime.write_text(header + body)

    def merged_seedlist(self) -> Iterable[str]:
        # Bootstrap node check: skip runtime seedlist to prevent outgoing connections
        import os
        # Find .env relative to this file's location
        script_dir = os.path.dirname(os.path.abspath(__file__))
        env_file = os.path.join(script_dir, '..', '.env')
        if os.path.exists(env_file):
            with open(env_file, "r") as f:
                if "BOOTSTRAP_NODE=true" in f.read():
                    # Return empty list for bootstrap nodes
                    return []
        
        baseline_entries = []
        if self.baseline.exists():
            for line in self.baseline.read_text().splitlines():
                line = line.strip()
                if not line or line.startswith("#"):
                    continue
                baseline_entries.append(line)
        combined = list(dict.fromkeys([*baseline_entries, *self._load_runtime_entries()]))
        return combined[: self.limit]


class PeerDirectory:
    """Persist peer observations and refresh the runtime seedlist."""

    def __init__(self, db_path: Path = PEER_DB_PATH, seed_manager: Optional[SeedlistManager] = None) -> None:
        self.db_path = db_path
        self.seed_manager = seed_manager or SeedlistManager()
        self.db_path.parent.mkdir(parents=True, exist_ok=True)
        self._records: Dict[str, PeerRecord] = {}
        self._load()

    def _load(self) -> None:
        if not self.db_path.exists():
            return
        try:
            raw = self.db_path.read_text().strip()
            if not raw:
                return
            data = json.loads(raw)
        except (OSError, json.JSONDecodeError):
            # Corrupt or empty peers file; start with a clean directory.
            return
        for multiaddr, payload in data.items():
            record = PeerRecord(
                multiaddr=multiaddr,
                agent=payload.get("agent"),
                first_seen=payload.get("first_seen", time.time()),
                last_seen=payload.get("last_seen", time.time()),
            )
            self._records[multiaddr] = record

    def _flush(self) -> None:
        serialised = {
            addr: {
                "agent": record.agent,
                "first_seen": record.first_seen,
                "last_seen": record.last_seen,
            }
            for addr, record in self._records.items()
        }
        self.db_path.write_text(json.dumps(serialised, indent=2, sort_keys=True))

    def register_connection(self, multiaddr: str, agent: Optional[str] = None) -> PeerRecord:
        entry = _normalize_multiaddr(multiaddr)
        record = self._records.get(entry)
        if record is None:
            record = PeerRecord(multiaddr=entry, agent=agent)
            self._records[entry] = record
        else:
            record.touch()
            if agent:
                record.agent = agent
        # Refresh runtime seedlist based on long-lived, healthy peers.
        # Seeds are:
        # - Non-miner agents
        # - Connected for at least 7 days
        # - Seen within the last 24 hours
        now = time.time()
        seven_days = 7 * 24 * 60 * 60
        stale_cutoff = 24 * 60 * 60
        eligible_entries = []
        for rec in self._records.values():
            if rec.agent and "miner" in rec.agent.lower():
                continue
            lifetime = now - rec.first_seen
            inactivity = now - rec.last_seen
            if lifetime >= seven_days and inactivity <= stale_cutoff:
                eligible_entries.append(rec.multiaddr)
        self.seed_manager.write_runtime_entries(eligible_entries)
        self._flush()
        return record

    def peers(self) -> Iterable[PeerRecord]:
        return sorted(self._records.values(), key=lambda record: record.last_seen, reverse=True)
