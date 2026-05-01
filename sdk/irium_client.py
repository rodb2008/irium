#!/usr/bin/env python3
"""
Irium Python SDK stub -- offer-take-proof-release cycle example.

All configuration is passed via environment variables or constructor arguments.
No hardcoded IP addresses, ports, or keys.

Usage:
    export IRIUM_NODE_URL=http://127.0.0.1:38300
    export IRIUM_RPC_TOKEN=<your-token>
    python3 irium_client.py
"""

import os
import json
import urllib.request
import urllib.error


class IriumClient:
    """Minimal Irium node API client. Pass base_url at runtime -- never hardcode."""

    def __init__(self, base_url=None, token=None):
        self.base_url = (base_url or os.environ.get("IRIUM_NODE_URL", "")).rstrip("/")
        self.token = token or os.environ.get("IRIUM_RPC_TOKEN", "")
        if not self.base_url:
            raise ValueError("base_url is required (or set IRIUM_NODE_URL)")

    def _post(self, path, payload):
        url = "{0}{1}".format(self.base_url, path)
        data = json.dumps(payload).encode()
        req = urllib.request.Request(url, data=data, headers={
            "Content-Type": "application/json",
            "Authorization": "Bearer {0}".format(self.token),
        })
        with urllib.request.urlopen(req, timeout=10) as resp:
            return json.loads(resp.read())

    def _get(self, path):
        url = "{0}{1}".format(self.base_url, path)
        req = urllib.request.Request(url, headers={
            "Authorization": "Bearer {0}".format(self.token),
        })
        with urllib.request.urlopen(req, timeout=10) as resp:
            return json.loads(resp.read())

    def status(self):
        """Get node status (height, peers)."""
        return self._get("/status")

    def list_offers(self):
        """Fetch the public offers feed."""
        return self._get("/offers/feed").get("offers", [])

    def submit_proof(self, proof):
        """Submit a settlement proof. Returns outcome with accepted/duplicate flags."""
        return self._post("/rpc/submitproof", {"proof": proof})

    def list_proofs(self, agreement_hash="*"):
        """List proofs, optionally filtered by agreement hash."""
        return self._post("/rpc/listproofs", {"agreement_hash": agreement_hash})

    def agreement_status(self, agreement_hash):
        """Get the status of an agreement by hash."""
        return self._post("/rpc/agreementstatus", {"agreement_hash": agreement_hash})

    def get_balance(self, address):
        """Get balance for an address."""
        return self._post("/rpc/balance", {"address": address})


def example_offer_take_proof_release_cycle():
    """
    Demonstrates the full OTC cycle:
    1. List available offers
    2. Take an offer (creates agreement)
    3. Submit proof of payment
    4. Check release eligibility

    In production: use irium-wallet CLI for key signing.
    The SDK client is stateless -- it only speaks to the node API.
    """
    client = IriumClient()  # reads IRIUM_NODE_URL and IRIUM_RPC_TOKEN from env

    print("=== Irium SDK Example: offer-take-proof-release ===")
    print()

    # Step 1: Check node status
    status = client.status()
    print("Node height: {0}".format(status.get("height", "N/A")))
    print("Peers: {0}".format(status.get("peer_count", 0)))
    print()

    # Step 2: List offers
    offers = client.list_offers()
    print("Available offers: {0}".format(len(offers)))
    if offers:
        o = offers[0]
        print("  First offer: {0} | {1} IRM | seller={2}...".format(
            o.get("title", "N/A"),
            o.get("amount_irm", "?"),
            o.get("seller", "?")[:20],
        ))
    print()

    print("Next steps (use irium-wallet CLI for signed operations):")
    print("  irium-wallet offer-take --offer <id> --buyer <addr> --rpc $IRIUM_NODE_URL")
    print("  irium-wallet agreement-proof-create --agreement-hash <hash> ...")
    print("  irium-wallet agreement-proof-submit --proof proof.json --rpc $IRIUM_NODE_URL")
    print("  irium-wallet agreement-policy-evaluate --agreement <hash> --rpc $IRIUM_NODE_URL")


if __name__ == "__main__":
    example_offer_take_proof_release_cycle()
