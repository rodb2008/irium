# Irium PoAW-X Public Testnet — Node Quickstart

This guide brings up an Irium **PoAW-X public testnet** node (and, optionally, a
solo miner) and connects it to the network. It is written to be followed verbatim
on a fresh Linux VM (e.g. an Ubuntu 24.04 LXC/VM on Proxmox).

> **Testnet only.** This is a test network. Coins have **no value**. Do not reuse a
> mainnet key anywhere here. Mainnet is a completely separate network and is never
> touched by this setup.

## Network facts

| | |
|---|---|
| Release tag | `poawx-testnet-v0.1.3` |
| Network | `devnet` magic (`network_id = 2`) |
| Genesis hash | `0000000028f25d65557e9d8d9e991f516c00d68f5aeae10b750645b398bd10a3` |
| Seed nodes (P2P) | `207.244.247.86:38401`, `157.173.116.134:38401` |
| P2P port | `38401/tcp` (must be reachable inbound) |
| RPC port | `38400/tcp` (loopback only — never expose) |
| Block reward split | 55 / 22 / 13 / 10 (PRIMARY / COMPUTE / VERIFY / SUPPORT) |

## 0. Requirements

- Linux **x86_64**, **glibc ≥ 2.39** (Ubuntu **24.04** recommended; the binaries are
  built against glibc 2.39). On older distros, run in an Ubuntu 24.04 container.
- ~2 vCPU, 2 GB RAM, 20 GB disk.
- One inbound TCP port: **38401** (forward it to the VM if behind NAT).
- `curl`, `tar`, `python3`, `openssl` installed.
- On **Debian 13**, `ufw` is not installed by default — install it first: `sudo apt install ufw` (used in step 3).

## 1. Download the software and verify it

> Always check https://github.com/iriumlabs/irium/releases for the latest testnet release and update the version number accordingly.

```bash
mkdir -p ~/testnet/bin ~/testnet/data && cd ~/testnet
BASE=https://github.com/iriumlabs/irium/releases/download/poawx-testnet-v0.1.3
curl -L -o bin/iriumd        $BASE/iriumd
curl -L -o bin/irium-miner   $BASE/irium-miner        # optional, only if you will mine
curl -L -o bootstrap.tar.gz  $BASE/bootstrap.tar.gz
curl -L -o testnet.env       $BASE/testnet.env
curl -L -o SHA256SUMS        $BASE/SHA256SUMS

# Verify integrity (MUST print "OK" for every file you downloaded)
( cd bin && sha256sum -c <(grep -E 'iriumd|irium-miner' ../SHA256SUMS) )
sha256sum -c <(grep -E 'bootstrap.tar.gz|testnet.env' SHA256SUMS)

chmod +x bin/iriumd bin/irium-miner
tar xzf bootstrap.tar.gz        # creates ~/testnet/bootstrap/anchors.json
```

## 2. Configure the node

The downloaded `testnet.env` holds the **canonical gate set** — every node must use
it unchanged, or the network will reject your blocks. Add your per-node settings:

```bash
cd ~/testnet
cp testnet.env iriumd.env
cat >> iriumd.env <<EOF
IRIUM_RPC_TOKEN=$(openssl rand -hex 32)
IRIUM_DATA_DIR=$HOME/testnet/data
IRIUM_NODE_CONFIG=$HOME/testnet/node.json
IRIUM_POAWX_RECEIPTS_FILE=$HOME/testnet/poawx_pending_receipts.json
EOF

cat > node.json <<EOF
{"p2p_bind":"0.0.0.0:38401","p2p_seeds":["207.244.247.86:38401","157.173.116.134:38401"],"data_dir":"$HOME/testnet/data"}
EOF
```

## 3. Firewall

```bash
sudo ufw allow 38401/tcp     # P2P inbound — required
# Do NOT open 38400 (RPC). It stays loopback-only.
```
If behind NAT (typical on Proxmox/home), forward external `38401/tcp` to this VM.

## 4. Run as a systemd service

> The service **working directory must contain `bootstrap/anchors.json`** or the node
> exits on startup. We set it to `~/testnet`.

```bash
sudo tee /etc/systemd/system/irium-testnet.service >/dev/null <<UNIT
[Unit]
Description=Irium PoAW-X Testnet Node
After=network-online.target
Wants=network-online.target
[Service]
User=$USER
WorkingDirectory=$HOME/testnet
EnvironmentFile=$HOME/testnet/iriumd.env
ExecStart=$HOME/testnet/bin/iriumd
Restart=always
RestartSec=3
[Install]
WantedBy=multi-user.target
UNIT
sudo systemctl daemon-reload
sudo systemctl enable --now irium-testnet
```

## 5. Verify it is syncing

```bash
T=$(grep '^IRIUM_RPC_TOKEN=' ~/testnet/iriumd.env | cut -d= -f2)
curl -s -H "Authorization: Bearer $T" http://127.0.0.1:38400/status | python3 -m json.tool
```
You should see, within ~1 minute:
- `"genesis_hash": "0000000028f2..."` (matches the table above),
- `"anchor_loaded": true`,
- `"peer_count"` ≥ 1,
- `"height"` rising over time and tracking the seeds,
- `"poawx_adaptive_mode"` one of `normal` / `caution` / `defense` / `recovery`.

Logs: `sudo journalctl -u irium-testnet -f`

## 6. (Optional) Mine with `irium-miner --poawx`

A solo miner plays **all roles** with its own key. Pool mining is not part of this
testnet phase.

```bash
cd ~/testnet
cp iriumd.env miner.env
cat >> miner.env <<EOF
IRIUM_NODE_RPC=http://127.0.0.1:38400
IRIUM_POAWX_MINER_SECRET_HEX=$(openssl rand -hex 32)
IRIUM_POAWX_MINER_INTERVAL_SECS=30
EOF
# (IRIUM_RPC_TOKEN is already inherited from iriumd.env — it must match your node.)
```
`IRIUM_POAWX_MINER_SECRET_HEX` is a **32-byte secret = 64 hex chars** (generated above
with `openssl rand -hex 32`). Block rewards go to the address derived from it.
`IRIUM_POAWX_MINER_INTERVAL_SECS=30` is the polite block cadence — please keep it ≥ 30.

Run it as a service:
```bash
sudo tee /etc/systemd/system/irium-testnet-miner.service >/dev/null <<UNIT
[Unit]
Description=Irium PoAW-X Testnet Solo Miner
After=irium-testnet.service
Requires=irium-testnet.service
[Service]
User=$USER
WorkingDirectory=$HOME/testnet
EnvironmentFile=$HOME/testnet/miner.env
ExecStart=$HOME/testnet/bin/irium-miner --poawx
Restart=always
RestartSec=5
[Install]
WantedBy=multi-user.target
UNIT
sudo systemctl daemon-reload
sudo systemctl enable --now irium-testnet-miner
sudo journalctl -u irium-testnet-miner -f   # expect: "submitted all-gates block height=N"
```

## 7. Troubleshooting

- **`Failed to load anchors`** → the service `WorkingDirectory` does not contain
  `bootstrap/anchors.json`. Fix the path or extract the bootstrap there.
- **`peer_count` stays 0** → 38401 inbound is blocked/not forwarded, or the seeds are
  unreachable from your network. Test: `nc -vz 207.244.247.86 38401`.
- **Height stuck far behind the seeds** → a node that falls a long way behind may not
  catch up via live gossip. Stop the node, delete `~/testnet/data/blocks`,
  `~/testnet/data/state`, `~/testnet/data/candidate_admissions.dat`, and restart to
  re-sync from genesis. (This is a known testnet sync limitation we are tracking.)
- **Miner: `admission ... HTTP 400/403`** → your node and miner env gate values
  differ from the network, or `IRIUM_RPC_TOKEN` differs between node and miner. Use
  the unchanged `testnet.env` for both and the same token.

## 8. Report your results

Please fill in `TESTNET_FEEDBACK.md` (in this same `docs/` directory) and open a
GitHub issue on `iriumlabs/irium` titled `[testnet] <your node name>`.
