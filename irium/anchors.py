"""Anchor file verification for eclipse attack protection."""

from __future__ import annotations

import hashlib
import json
import logging
import subprocess
import tempfile
import textwrap
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional

logger = logging.getLogger(__name__)


class AnchorVerificationError(RuntimeError):
    """Raised when anchor data fails signature validation."""


def _canonical_payload_bytes(data: dict) -> bytes:
    payload = dict(data)
    payload.pop("signatures", None)
    return json.dumps(payload, separators=(",", ":"), sort_keys=True).encode()


def _signature_file_contents(signature_b64: str) -> str:
    wrapped = textwrap.fill(signature_b64, 70)
    return (
        "-----BEGIN SSH SIGNATURE-----\n"
        f"{wrapped}\n"
        "-----END SSH SIGNATURE-----\n"
    )


@dataclass
class AnchorHeader:
    """Checkpoint header in anchor file."""

    height: int
    hash: str
    timestamp: int
    prev_hash: str

    def to_dict(self) -> dict:
        return {
            "height": self.height,
            "hash": self.hash,
            "timestamp": self.timestamp,
            "prev_hash": self.prev_hash,
        }

    @classmethod
    def from_dict(cls, data: dict) -> AnchorHeader:
        return cls(
            height=data["height"],
            hash=data["hash"],
            timestamp=data["timestamp"],
            prev_hash=data["prev_hash"],
        )


class AnchorManager:
    """Manage and verify anchor file checkpoints."""

    def __init__(self, anchors_file: str = "bootstrap/anchors.json"):
        self.anchors_file = Path(anchors_file)
        self.anchors: List[AnchorHeader] = []
        self.trusted_signers: List[str] = []
        self.signatures: List[dict] = []
        self.payload_digest: Optional[str] = None
        self.verified_by: Optional[str] = None
        self._metadata: dict = {}
        self._load()

    def _load(self) -> None:
        if not self.anchors_file.exists():
            logger.warning("Anchor file %s not found; eclipse protection disabled", self.anchors_file)
            return

        try:
            raw_data = json.loads(self.anchors_file.read_text())
        except json.JSONDecodeError as exc:
            raise AnchorVerificationError(f"Invalid JSON in {self.anchors_file}: {exc}") from exc

        canonical = _canonical_payload_bytes(raw_data)
        self.signatures = raw_data.get("signatures", [])
        self.trusted_signers = raw_data.get("trusted_signers", [])
        if not self.trusted_signers:
            raise AnchorVerificationError("No trusted_signers defined in anchor file")

        valid_entry = self._verify_signatures(raw_data, canonical)
        self.payload_digest = hashlib.sha256(canonical).hexdigest()
        self.verified_by = valid_entry.get("signer") if valid_entry else None

        self.anchors = [AnchorHeader.from_dict(item) for item in raw_data.get("anchors", [])]
        self.anchors.sort(key=lambda x: x.height)
        self._metadata = {k: v for k, v in raw_data.items() if k not in {"anchors", "signatures"}}
        logger.info(
            "Loaded %s anchors (digest=%s, signer=%s)",
            len(self.anchors),
            self.payload_digest,
            self.verified_by,
        )

    def _verify_signatures(self, data: dict, canonical: bytes) -> dict:
        if not self.signatures:
            raise AnchorVerificationError("Anchor file contains no signatures")

        trusted = set(self.trusted_signers)
        last_error: Optional[str] = None
        for entry in self.signatures:
            signer = entry.get("signer")
            if signer not in trusted:
                continue
            if self._verify_entry(entry, canonical):
                return entry
            last_error = f"signature from {signer} failed verification"

        msg = last_error or "no signatures from trusted signers"
        raise AnchorVerificationError(msg)

    def _verify_entry(self, entry: dict, canonical: bytes) -> bool:
        namespace = entry.get("namespace", "irium-anchor")
        signature = entry.get("signature")
        public_key = entry.get("public_key")
        signer = entry.get("signer", "unknown")

        if not signature or not public_key:
            return False

        with tempfile.TemporaryDirectory() as tmpdir:
            tmp_path = Path(tmpdir)
            sig_file = tmp_path / "payload.sig"
            allowed_file = tmp_path / "allowed_signers"

            sig_file.write_text(_signature_file_contents(signature))
            allowed_line = f"{signer} namespaces=\"{namespace}\" {public_key.strip()}\n"
            allowed_file.write_text(allowed_line)

            cmd = [
                "ssh-keygen",
                "-Y",
                "verify",
                "-f",
                str(allowed_file),
                "-I",
                signer,
                "-n",
                namespace,
                "-s",
                str(sig_file),
            ]
            try:
                subprocess.run(cmd, check=True, capture_output=True, input=canonical)
                return True
            except FileNotFoundError as exc:
                raise AnchorVerificationError("ssh-keygen is required to verify anchors") from exc
            except subprocess.CalledProcessError as exc:
                stderr = exc.stderr.decode().strip() if exc.stderr else "verification failed"
                logger.debug("Anchor signature verification failed: %s", stderr)
                return False

    def get_anchor_at_height(self, height: int) -> Optional[AnchorHeader]:
        for anchor in self.anchors:
            if anchor.height == height:
                return anchor
        return None

    def get_latest_anchor(self) -> Optional[AnchorHeader]:
        return self.anchors[-1] if self.anchors else None

    def verify_block_against_anchors(self, height: int, block_hash: str) -> bool:
        anchor = self.get_anchor_at_height(height)
        if anchor is None:
            return True
        return anchor.hash == block_hash

    def is_chain_valid(self, chain_tip_height: int, chain_tip_hash: str) -> bool:
        relevant_anchor = None
        for anchor in reversed(self.anchors):
            if anchor.height <= chain_tip_height:
                relevant_anchor = anchor
                break

        if not relevant_anchor:
            return True

        if chain_tip_height == relevant_anchor.height:
            return chain_tip_hash == relevant_anchor.hash

        # TODO: integrate with full header validation once headers DB is available.
        return True

    def add_anchor(self, anchor: AnchorHeader, signature: Optional[str] = None) -> None:
        existing = self.get_anchor_at_height(anchor.height)
        if existing:
            if existing.hash != anchor.hash:
                logger.warning("Conflicting anchor at height %s", anchor.height)
            return

        self.anchors.append(anchor)
        self.anchors.sort(key=lambda x: x.height)
        self._save()

    def _save(self) -> None:
        data = dict(self._metadata)
        data["trusted_signers"] = self.trusted_signers
        data["anchors"] = [anchor.to_dict() for anchor in self.anchors]
        if self.signatures:
            data["signatures"] = self.signatures

        try:
            with open(self.anchors_file, "w") as fp:
                json.dump(data, fp, indent=2)
        except OSError as exc:
            logger.error("Error saving anchors: %s", exc)

    def get_stats(self) -> dict:
        if not self.anchors:
            return {
                "total_anchors": 0,
                "latest_height": 0,
                "trusted_signers": len(self.trusted_signers),
            }

        return {
            "total_anchors": len(self.anchors),
            "latest_height": self.anchors[-1].height,
            "latest_hash": self.anchors[-1].hash,
            "trusted_signers": len(self.trusted_signers),
        }


class EclipseProtection:
    """Protect against eclipse attacks using anchors."""

    def __init__(self, anchor_manager: AnchorManager):
        self.anchor_manager = anchor_manager
        self.suspicious_peers: set[str] = set()

    def verify_peer_chain(self, peer_id: str, peer_height: int, peer_tip_hash: str) -> bool:
        if not self.anchor_manager.is_chain_valid(peer_height, peer_tip_hash):
            logger.warning("Peer %s has chain inconsistent with anchors", peer_id)
            self.suspicious_peers.add(peer_id)
            return False
        return True

    def is_peer_suspicious(self, peer_id: str) -> bool:
        return peer_id in self.suspicious_peers

    def clear_suspicion(self, peer_id: str) -> None:
        self.suspicious_peers.discard(peer_id)
