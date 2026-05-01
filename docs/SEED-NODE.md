# Irium Seed Node Operator Guide

## Overview

Seed nodes are the entry points new nodes use to join the Irium network. A fresh installation reads `bootstrap/seedlist.txt`, connects to the listed IPs, and discovers the rest of the network from there. The current seedlist has two nodes. Adding more reduces the risk of a new node being unable to bootstrap if one seed is temporarily offline.

This guide covers running a permanent publicly reachable seed node and applying to be added to the official seedlist.

## Minimum Requirements

| Resource | Minimum |
|----------|---------|
| CPU | 1 core |
| RAM | 1 GB |
| Disk | 20 GB (for chain data) |
| Upload | 10 Mbps sustained |
| IP | Static, publicly reachable |
| OS | Ubuntu 22.04+ or Debian 12+ |
| Uptime commitment | 95%+ monthly |

## What a Seed Node Does

A seed node runs a standard `iriumd` instance with its P2P port (default 38291) open to the internet. It does not require any special configuration beyond being publicly reachable. Any fully-synced iriumd instance with an open P2P port qualifies.

The seed node does not need to run a mining pool or expose an RPC API publicly. Only the P2P port needs to be reachable.

## Setup

### 1. Install iriumd

```bash
# Build from source
git clone https://github.com/iriumlabs/irium.git
cd irium
cargo build --release --bin iriumd

# Or download a pre-built binary from GitHub Releases
# https://github.com/iriumlabs/irium/releases/latest
```

### 2. Configure

```bash
# Minimum env for a seed node
export IRIUM_NODE_CONFIG=configs/node.json
export IRIUM_P2P_BIND=0.0.0.0:38291
export IRIUM_NODE_PORT=38300
export IRIUM_NODE_HOST=127.0.0.1
export IRIUM_STATUS_PORT=8080
```

No RPC token is required if you keep the RPC listener on loopback (`127.0.0.1`).

### 3. Open firewall port

The P2P port must be reachable from the internet. Port 38291 is the default; it is configurable via `IRIUM_P2P_BIND`.

```bash
# UFW example
sudo ufw allow 38291/tcp comment "Irium P2P"

# iptables example
sudo iptables -I INPUT -p tcp --dport 38291 -j ACCEPT
```

### 4. Run as a systemd service

```bash
sudo tee /etc/systemd/system/iriumd.service > /dev/null << 'EOF'
[Unit]
Description=Irium Seed Node
After=network-online.target
Wants=network-online.target

[Service]
User=irium
WorkingDirectory=/home/irium/irium
EnvironmentFile=-/etc/irium/iriumd.env
ExecStart=/home/irium/irium/target/release/iriumd
Restart=on-failure
RestartSec=5
TimeoutStopSec=90
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable --now iriumd
```

Place your environment variables in `/etc/irium/iriumd.env`.

### 5. Verify your node is publicly reachable

From a different machine:
```bash
nc -zv YOUR_SERVER_IP 38291
```

Or confirm via the status endpoint from the server itself:
```bash
curl http://localhost:8080/status | python3 -m json.tool | grep peer_count
```

A seed node should maintain at least 3 connected peers under normal network conditions.

## Applying to Join the Official Seedlist

When your node has been running stably for at least 7 days, apply to have your IP added to `bootstrap/seedlist.txt`.

**Information to provide:**
- Your IP address and P2P port
- Current node version (`iriumd --version` or from `/status`)
- Expected monthly uptime commitment (%)
- Contact method (Telegram handle or GitHub username)

**Where to apply:**
- Telegram: https://t.me/iriumlabs
- GitHub Issue: https://github.com/iriumlabs/irium/issues (title: "Seed node application: [your IP]")

Ibrahim reviews applications and verifies each node is publicly reachable before adding it to the seedlist.

**Verification before listing:** Ibrahim will run the following to confirm your P2P port is reachable and your node is synced to the current chain tip:
```bash
nc -zv YOUR_IP YOUR_PORT
curl http://YOUR_IP:8080/status
```

## How the Seedlist Signature Works

`bootstrap/seedlist.txt` is cryptographically signed with an SSH Ed25519 key. The node verifies the signature on startup and logs a warning if verification fails (it will still use the seeds, but the warning indicates tampering or a key change).

The signing key's public key is in `bootstrap/trust/allowed_signers`:
```
bootstrap-signer ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAID1kiQ+hbA8URyVCGDLjeDGcN18rBBURyBH6xppP9qsk info@iriumlabs.org
```

### Verifying the signature yourself

To independently verify the current seedlist signature:
```bash
ssh-keygen -Y verify \
  -f bootstrap/trust/allowed_signers \
  -I bootstrap-signer \
  -n file \
  -s bootstrap/seedlist.txt.sig \
  < bootstrap/seedlist.txt
```

Expected output when valid:
```
Good "file" signature for bootstrap-signer with ED25519 key SHA256:nmx/FolVmEcSar4I+A4qdF1BBvmh8ddxzxZ51kQzTeg
```

The fingerprint `SHA256:nmx/FolVmEcSar4I+A4qdF1BBvmh8ddxzxZ51kQzTeg` is the canonical fingerprint of the Irium Labs bootstrap signing key. Any valid seedlist will verify against this exact fingerprint.

## Conditions for Removal

An IP is removed from the seedlist if:
- The node is unreachable for more than 14 consecutive days
- The node is serving incorrect chain data (wrong genesis hash or forked chain)
- The operator requests removal
- The node is confirmed to be acting adversarially (e.g. sending invalid headers to poison new nodes)

Removal is done by re-signing the seedlist without the IP and pushing to the repository. Node software will pick up the updated list on next restart.

## Contact

Telegram: https://t.me/iriumlabs
GitHub Issues: https://github.com/iriumlabs/irium
