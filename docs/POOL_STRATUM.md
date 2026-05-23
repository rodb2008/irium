## Source Repository

The Stratum pool server is built from the main Irium repository:
https://github.com/iriumlabs/irium

The Stratum binaries (`irium-stratum` and `irium-stratum-legacy`) are compiled
from the same source as iriumd. Build with:

    cd irium
    cargo build --release

The binaries will be at `target/release/irium-stratum` and
`target/release/irium-stratum-legacy`.

# Irium Public Stratum Pool

## Endpoint status

`pool.iriumlabs.org` is the intended public Stratum DNS hostname, not a website. Before announcing the migrated pool, operators must confirm DNS points at the active pool host and both TCP ports are listening.

## Quickstart (miners)
Port profiles:

- `3333` (strict canonical): `stratum+tcp://pool.iriumlabs.org:3333`
  - Use for: ASIC/modern firmware (Bitaxe, S19, S21, Whatsminer, Avalon)
- `3335` (legacy compatibility): `stratum+tcp://pool.iriumlabs.org:3335`
  - Use for: CPU/GPU and older Stratum clients (cpuminer-opt, ccminer, T-Rex, lolMiner, NBMiner, legacy cgminer family)
- `443` (ISP-block fallback): `stratum+tcp://pool.iriumlabs.org:443`
  - Use when: outbound TCP/3333 or TCP/3335 is filtered by your ISP (notably common in mainland China). Same Stratum protocol served on the HTTPS port to bypass DPI filtering.
- `80` (second fallback): `stratum+tcp://pool.iriumlabs.org:80`
  - Use when: both 3333/3335 and 443 are filtered. Same protocol, HTTP port.
- Username: `IRM_ADDRESS.worker1`
- Password: `x`

Example username:
- `Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa.worker1`

Live stats: [https://pool.iriumlabs.org/stats](https://pool.iriumlabs.org/stats)
(active miners, accepted/rejected shares, current share difficulty per
profile, blocks found, rolling-window hashrate estimate).

**Block 23,500 hard fork (Fix 2a):** the chain activates Bitcoin-
standard block-header serialization at block 23,500. After activation
every SHA-256d miner — Bitaxe, ASIC, T-Rex, lolMiner, NBMiner,
cpuminer-opt — that submits a winning share earns the full block
reward (currently 50 IRM) directly to the IRM address used as the
Stratum worker name. This pool runs in SOLO payout mode; there is no
pool fee.

## Recommended endpoint selection

- ASIC/modern firmware: use port `3333`
- CPU/GPU/legacy clients: use port `3335`

Recommended failover list for CPU/GPU/legacy clients:
1. `stratum+tcp://pool.iriumlabs.org:3335`
2. `stratum+tcp://pool.iriumlabs.org:3333`

Recommended failover list for ASIC/strict clients:
1. `stratum+tcp://pool.iriumlabs.org:3333`
2. `stratum+tcp://pool.iriumlabs.org:3335`

Use the DNS hostname only after operator cutover; backend IPs may change and should not be published in public miner configuration.

## Payout model (SOLO)
This pool runs in SOLO mode.

- Your username must start with your IRM address.
- If your share finds a valid network block, the block reward is paid to that IRM address.
- Worker suffix (for example `.worker1`) is only for rig identification.

## Connectivity troubleshooting
If you cannot connect:

1. Check DNS resolution:
```bash
getent hosts pool.iriumlabs.org
```

2. Check TCP reachability on both public ports:
```bash
nc -vz pool.iriumlabs.org 3333
nc -vz pool.iriumlabs.org 3335
```

3. Test a raw Stratum subscribe (strict + legacy):
```bash
printf '{"id":1,"method":"mining.subscribe","params":[]}\n' | nc pool.iriumlabs.org 3333
printf '{"id":1,"method":"mining.subscribe","params":[]}\n' | nc pool.iriumlabs.org 3335
```

4. If DNS fails:
- Fix resolver settings on the miner network.
- Do not publish or depend on backend host IPs in public miner config.

5. If connectivity still fails:
- Confirm outbound TCP/3333 and TCP/3335 are allowed on firewall/router.
- Check ISP/VPS filtering for mining ports.
- Try another network to isolate local filtering.

## How to verify service is up
From any Linux/macOS shell:
```bash
nc -vz pool.iriumlabs.org 3333
nc -vz pool.iriumlabs.org 3335
```

From the pool host:
```bash
systemctl status irium-stratum --no-pager
systemctl status irium-stratum-legacy --no-pager
ss -lntp | egrep ':3333|:3335|:3334|:3336'
```

## Operator notes

### Required ports
- Public strict: `3333/tcp` (Stratum strict/ASIC)
- Public legacy: `3335/tcp` (Stratum legacy CPU/GPU)
- Node P2P: `38291/tcp` (if this host also runs a node)
- Keep node RPC private: `127.0.0.1:38300` only

### Service control
```bash
sudo systemctl restart irium-stratum
sudo systemctl restart irium-stratum-legacy
sudo systemctl status irium-stratum --no-pager
sudo systemctl status irium-stratum-legacy --no-pager
journalctl -u irium-stratum -f
journalctl -u irium-stratum-legacy -f
```

### Config locations
- Strict env: `/etc/irium-pool/stratum.env`
- Legacy env: `/etc/irium-pool/stratum-legacy.env`
- Service units: `/etc/systemd/system/irium-stratum.service`, `/etc/systemd/system/irium-stratum-legacy.service`
- Source: `/opt/irium-pool/irium-stratum`

### Security requirements
- Keep `/etc/irium-pool/stratum.env` and `/etc/irium-pool/stratum-legacy.env` mode `600`.
- Do not expose `127.0.0.1:38300` publicly.
- Keep `IRIUM_RPC_TOKEN` secret.

## Common log messages
- `[tmpl] fetch failed ... operation timed out`
: Temporary local RPC/connect stall. Stratum retries automatically.

- `[conn] bad json ...`
: Non-Stratum or malformed traffic hit a public Stratum port.

- `[share] reject ... reason=low_difficulty`
: Share did not meet current pool difficulty target.

- `[block] submitted ...`
: A block candidate met network target and was submitted to the node.

## Disclaimer
Current implementation has been validated with local Stratum handshake and submit-path testing. Broader validation with real ASIC miners is welcome.

## Legacy client routing (March 6, 2026)
Use explicit port routing by hardware/client type:

- `3333` strict canonical profile: ASIC/modern firmware
- `3335` legacy profile: CPU/GPU and older Stratum clients

If the public hostname connects but your miner still reports subscribe/authorize issues, verify:
- Algorithm is SHA-256/SHA-256d
- SSL/TLS is disabled (`stratum+tcp://`)
- Worker format is `IRM_ADDRESS.worker1`
- You selected the correct port profile (`3333` vs `3335`)

## Miner Compatibility Matrix

| Miner | Class | Status | Known-good launch hint | Notes |
|---|---|---|---|---|
| `cpuminer-opt 26.1` | CPU | Stable on legacy port | `-a sha256d -o stratum+tcp://pool.iriumlabs.org:3335 -u WALLET.worker -p x -t N` | Use legacy profile port `3335`. |
| `ccminer 2.3.1/2.3.2` | GPU | Stable on legacy port | `-a sha256d -o stratum+tcp://pool.iriumlabs.org:3335 -u WALLET.worker -p x` | Use legacy profile port `3335`. |
| `T-Rex` | GPU | Validated | `t-rex -a sha256d -o stratum+tcp://pool.iriumlabs.org:3335 -u WALLET.gpu1 -p x` | Use legacy profile port `3335`. NVIDIA-only. |
| `lolMiner` | GPU | Validated | `lolMiner --algo SHA256D --pool stratum+tcp://pool.iriumlabs.org:3335 --user WALLET.gpu1 --pass x` | Use legacy profile port `3335`. AMD + NVIDIA. |
| `NBMiner` | GPU | Validated | `nbminer -a sha256d -o stratum+tcp://pool.iriumlabs.org:3335 -u WALLET.gpu1 -p x` | Use legacy profile port `3335`. |
| `Bitaxe` (S19-compatible firmware) | ASIC | Validated | Stratum URL `pool.iriumlabs.org`, Port `3333`, User `WALLET.bitaxe1`, Password `x`, TLS OFF | Use strict port `3333`. Switch to `443` if your ISP blocks `3333`. |
| `cgminer / bmminer 4.10.0` | ASIC/legacy | Stable on strict port | Pool0 DNS hostname | Use strict port `3333`; fallback `3335` only if firmware requires it. |
| `S19 / S21 / Whatsminer / Avalon` (stock firmware) | ASIC | Validated | Pool URL `stratum+tcp://pool.iriumlabs.org:3333`, Worker `WALLET.rig1`, Password `x` | Same as Bitaxe; switch to port `443` if 3333 is blocked. |
| `irium-miner` / `irium-miner-gpu` | Native | Recommended | Use latest `main` build | Baseline for protocol correctness and debugging. |

Use `/metrics` reject reasons (`rejected_stale`, `rejected_low_difficulty`, `rejected_invalid`, `rejected_duplicate`) for triage.


---

## Supported miners (v1.9.24+)

As of v1.9.24 the Stratum server handles `mining.configure` (version-rolling negotiation), `mining.suggest_difficulty`, and `mining.multi_version` so the following firmware connects without protocol-error disconnects.

### ASIC miners (port 3333)

| Miner | Chip(s) | Firmware | Status |
|---|---|---|---|
| **Bitaxe** | BM1366 / BM1368 | ESP-Miner / AxeOS v1.x and v2.x | Supported. v2+ requires `mining.configure` (shipped). |
| **Antminer** | S19 series, S21 series | bmminer (with or without version-rolling) | Supported. AsicBoost auto-disabled by pool. |
| **Whatsminer** | M30 series, M50 series | BTMiner | Supported. `mining.multi_version` is acknowledged so older builds do not disconnect. |
| **Avalon** | A12 series, A13 series | CanaanMiner / AvalonMiner | Supported. Standard Stratum v1. |

### CPU / GPU miners (port 3335)

| Miner | Class | Notes |
|---|---|---|
| **cgminer** (incl. `bmminer`/`bfgminer`) | CPU/ASIC | Reference Stratum v1 implementation. Works on either port. |
| **cpuminer-opt 26.1** | CPU | Use `-a sha256d`. |
| **ccminer 2.3.x** | GPU (CUDA) | Use `-a sha256d`. |
| **lolMiner** | GPU (OpenCL/CUDA) | SHA-256d profile. |
| **T-Rex** | GPU (CUDA) | SHA-256d profile. |
| **NBMiner** | GPU | SHA-256d profile. |
| **irium-miner** | Native | Baseline. |

XMRig is not applicable (RandomX algorithm, not SHA-256d).

---

## Pool connection guide

| | |
|---|---|
| **Pool address** | `pool.iriumlabs.org` |
| **ASIC port** | `3333/tcp` (strict canonical profile, default difficulty 16) |
| **CPU/GPU port** | `3335/tcp` (legacy compat profile, default difficulty 1) |
| **Stats endpoint** | `http://pool.iriumlabs.org:3337/` |
| **Algorithm** | SHA-256d (double SHA-256) |
| **Protocol** | Stratum v1 only. Stratum v2 not supported; miners fall back to v1 cleanly. |
| **Version rolling (AsicBoost)** | Not supported. Firmware that calls `mining.configure` is told `"version-rolling": false` and auto-disables AsicBoost. |
| **Username format** | `your_irium_address.worker_name` (worker suffix optional, for rig identification) |
| **Password** | Any non-empty string (commonly `x`) |
| **Vardiff** | Enabled. Starts at difficulty 16 (ASIC) or 1 (CPU/GPU). Retargets every 30 s to a 15 s share interval. Range: 1 to 2048 (ASIC) / 1024 (CPU/GPU). |
| **TLS** | Disabled. Connect with plain `stratum+tcp://`, not `stratum+ssl://`. |

### Example username (mainnet IRM address)

```
Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa.rig01
```

### Note for Chinese-region miners

If port 3333 is blocked by your ISP, try a VPN to reach the pool, or contact the operators for alternative connection options. The Stratum protocol is plain TCP and does not negotiate any encryption above the transport.

---

## Miner-specific quickstart

### Bitaxe (AxeOS / ESP-Miner)

In the AxeOS web UI:

| Field | Value |
|---|---|
| Stratum URL | `pool.iriumlabs.org` |
| Stratum Port | `3333` |
| Worker (Stratum User) | `Q...your_irium_address.bitaxe01` |
| Password | `x` |

Save and reboot. AxeOS will negotiate `mining.configure`, receive `version-rolling: false`, disable AsicBoost, and begin submitting shares at diff 16 (vardiff will retune).

### Antminer (bmminer-family)

In the miner web UI Pool 1 settings:

| Field | Value |
|---|---|
| URL | `stratum+tcp://pool.iriumlabs.org:3333` |
| Worker | `Q...your_irium_address.antminer01` |
| Password | `x` |

If your firmware insists on version-rolling, that's fine — the pool tells it `false` and bmminer disables it.

### Whatsminer (BTMiner)

In the BTMiner web UI:

| Field | Value |
|---|---|
| Pool 1 URL | `stratum+tcp://pool.iriumlabs.org:3333` |
| Pool 1 Worker | `Q...your_irium_address.whatsminer01` |
| Pool 1 Password | `x` |

`mining.multi_version` is acknowledged with `false`; the miner falls back to single-version templates.

### cpuminer-opt (CPU)

```bash
cpuminer -a sha256d \
  -o stratum+tcp://pool.iriumlabs.org:3335 \
  -u Q...your_irium_address.cpurig \
  -p x \
  -t 4
```

### ccminer / lolMiner / T-Rex / NBMiner (GPU SHA-256d)

```bash
ccminer -a sha256d \
  -o stratum+tcp://pool.iriumlabs.org:3335 \
  -u Q...your_irium_address.gpurig \
  -p x
```

Same address + port for the other GPU clients; consult each miner's manual for the `--algorithm` flag and check `-a sha256d` is the SHA-256 double-hash variant (not single).

---

## Changelog

For the full per-release pool-Stratum changelog (including the v1.9.24
firmware-compatibility additions — `mining.configure`,
`mining.suggest_difficulty`, `mining.multi_version`, and the
`mining.subscribe` `extranonce2_size` fix), see the GitHub releases page:

[github.com/iriumlabs/irium/releases](https://github.com/iriumlabs/irium/releases)
