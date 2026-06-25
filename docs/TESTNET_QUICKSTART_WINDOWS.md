# Irium PoAW-X Public Testnet — Windows Quickstart

Run an Irium **PoAW-X public testnet** node (and, optionally, a solo miner) on
Windows 10/11 using PowerShell. For Linux see `TESTNET_QUICKSTART.md`.

> **Testnet only.** Coins have **no value**. Do not reuse a mainnet key. Mainnet is a
> separate network and is never touched by this setup.

## Network facts

| | |
|---|---|
| Release tag | `poawx-testnet-v0.1.3` |
| Network | `devnet` magic (`network_id = 2`) |
| Genesis hash | `0000000028f25d65557e9d8d9e991f516c00d68f5aeae10b750645b398bd10a3` |
| Seed nodes (P2P) | `207.244.247.86:38401`, `157.173.116.134:38401` |
| P2P port | `38401/tcp` (inbound) |
| RPC port | `38400/tcp` (loopback only — never expose) |
| Reward split | 55 / 22 / 13 / 10 (PRIMARY / COMPUTE / VERIFY / SUPPORT) |

All commands below are **PowerShell** (not cmd.exe). Open PowerShell and pick **one**
of the two ways to get the binaries (§1A download, or §1B build).

---

## 1A. Get the binaries — download (easiest)

> Always check https://github.com/iriumlabs/irium/releases for the latest testnet release and update the version number accordingly.

Download the Windows archive + bootstrap from the release, then verify and extract.

```powershell
$base = "https://github.com/iriumlabs/irium/releases/download/poawx-testnet-v0.1.3"
New-Item -ItemType Directory -Force "$HOME\irium-testnet" | Out-Null
Set-Location "$HOME\irium-testnet"

# node/miner binaries (Windows zip) + bootstrap + the canonical env + checksums
Invoke-WebRequest "$base/irium-poawx-testnet-v0.1.3-windows-x86_64.zip" -OutFile irium-win.zip
Invoke-WebRequest "$base/SHA256SUMS-windows.txt"                       -OutFile SHA256SUMS-windows.txt
Invoke-WebRequest "$base/bootstrap.tar.gz"                              -OutFile bootstrap.tar.gz
Invoke-WebRequest "$base/testnet.env"                                   -OutFile testnet.env

# verify the zip checksum (must match the published value)
(Get-FileHash irium-win.zip -Algorithm SHA256).Hash.ToLower()
Get-Content SHA256SUMS-windows.txt    # compare the line for the .zip

# extract (Windows 10+ ships 'tar' and 'Expand-Archive')
Expand-Archive .\irium-win.zip -DestinationPath . -Force   # -> iriumd.exe, irium-miner.exe, ...
tar xzf .\bootstrap.tar.gz                                  # -> .\bootstrap\anchors.json
```

You now have `iriumd.exe` and `irium-miner.exe` plus `.\bootstrap\anchors.json` in
`$HOME\irium-testnet`. Skip to §2.

> If the Windows zip is not yet attached to the release, use §1B (build from source).

## 1B. Get the binaries — build from source (you have Rust)

Requires `rustup` (MSVC toolchain — the Windows default) and the Visual Studio C++
build tools (for `link.exe`). No GPU/OpenCL is needed for these two binaries.

```powershell
git clone https://github.com/iriumlabs/irium.git
Set-Location irium
git checkout testnet/poawx-phase20-blueprint-completion-local
cargo build --release --bin iriumd --bin irium-miner
# binaries: .\target\release\iriumd.exe  and  .\target\release\irium-miner.exe
# the repo already contains .\bootstrap\anchors.json
```

When building from source, run the node **from the repo root** (so it finds
`.\bootstrap\anchors.json`). The commands in §3-§6 assume your working directory
contains both the `iriumd.exe` you will run and a `bootstrap\anchors.json`. From a
source build, either run from the repo root using `.\target\release\iriumd.exe`, or
copy the two `.exe` files and the `bootstrap` folder into `$HOME\irium-testnet`.

---

## 2. Create the config files

From your working directory (`$HOME\irium-testnet` for the download path, or the repo
root for the source path):

```powershell
# node.json — bind P2P, list the seed nodes (forward slashes are fine in JSON on Windows)
$dataDir = (Join-Path (Get-Location) "data") -replace '\\','/'
@"
{"p2p_bind":"0.0.0.0:38401","p2p_seeds":["207.244.247.86:38401","157.173.116.134:38401"],"data_dir":"$dataDir"}
"@ | Set-Content -Encoding ascii .\node.json
New-Item -ItemType Directory -Force .\data | Out-Null
Get-Content .\node.json
```

## 3. Load the testnet environment (PowerShell session)

`iriumd` reads its configuration from environment variables (there is no auto-loaded
`.env` file). This snippet loads the canonical gate set from `testnet.env`, then adds
your per-node values. Run it in the **same PowerShell window** you will start the node
from.

```powershell
# load the canonical gate set (IRIUM_* lines) into this session
Get-Content .\testnet.env | Where-Object { $_ -match '^\s*IRIUM_[A-Z0-9_]+=' } | ForEach-Object {
    $k,$v = $_ -split '=',2
    Set-Item -Path ("Env:" + $k.Trim()) -Value $v.Trim()
}

# per-node values
$env:IRIUM_RPC_TOKEN           = -join (1..32 | ForEach-Object { '{0:x2}' -f (Get-Random -Maximum 256) })
$env:IRIUM_DATA_DIR            = (Join-Path (Get-Location) "data")
$env:IRIUM_NODE_CONFIG         = (Join-Path (Get-Location) "node.json")
$env:IRIUM_POAWX_RECEIPTS_FILE = (Join-Path (Get-Location) "poawx_pending_receipts.json")

# save your token so you can query the node later
$env:IRIUM_RPC_TOKEN | Set-Content -Encoding ascii .\rpc_token.txt
"network=$($env:IRIUM_NETWORK)  rpc=$($env:IRIUM_NODE_HOST):$($env:IRIUM_NODE_PORT)"
```

> Keep gate values exactly as shipped in `testnet.env`. A single mismatch (e.g. a
> different finality threshold or sybil-bits) makes the network reject your blocks.

## 4. Firewall — allow inbound P2P 38401

Run PowerShell **as Administrator**:

```powershell
New-NetFirewallRule -DisplayName "Irium Testnet P2P 38401" -Direction Inbound `
  -Protocol TCP -LocalPort 38401 -Action Allow
# Do NOT open 38400 (RPC). It stays loopback-only.
```
If this VM is behind a router/NAT, also forward external `38401/tcp` to it.

## 5. Run the node

In the PowerShell window where you loaded the environment (§3):

```powershell
# download path:
.\iriumd.exe
# source build, from the repo root instead:
# .\target\release\iriumd.exe
```
Leave it running. To run it minimized in the background later, use a second window or a
Scheduled Task; for the testnet, a dedicated PowerShell window is fine.

## 6. Verify it is syncing

In a **second** PowerShell window, in the same folder:

```powershell
$T = Get-Content .\rpc_token.txt
$r = Invoke-RestMethod -Uri http://127.0.0.1:38400/status -Headers @{ Authorization = "Bearer $T" }
$r | Select-Object height, peer_count, anchor_loaded, genesis_hash, poawx_adaptive_mode | Format-List
```
Within ~1 minute you should see:
- `genesis_hash` = `0000000028f2...` (matches the table),
- `anchor_loaded` = `True`,
- `peer_count` >= 1,
- `height` rising and tracking the seeds,
- `poawx_adaptive_mode` one of `normal` / `caution` / `defense` / `recovery`.

Re-run the command to watch the height climb. If `peer_count` stays 0, check that
38401 inbound is allowed and the seeds are reachable: `Test-NetConnection 207.244.247.86 -Port 38401`.

## 7. (Optional) Mine with `irium-miner --poawx`

A solo miner plays all roles with its own key. In a **new** PowerShell window, from the
same folder, load the environment again (§3) — or just the gate set — then:

```powershell
# (after loading testnet.env gate vars and IRIUM_RPC_TOKEN as in §3)
$env:IRIUM_NODE_RPC               = "http://127.0.0.1:38400"
$env:IRIUM_POAWX_MINER_SECRET_HEX = -join (1..32 | ForEach-Object { '{0:x2}' -f (Get-Random -Maximum 256) })
$env:IRIUM_POAWX_MINER_INTERVAL_SECS = "30"   # polite cadence; keep >= 30
.\irium-miner.exe --poawx
# source build: .\target\release\irium-miner.exe --poawx
```
`IRIUM_POAWX_MINER_SECRET_HEX` is a 32-byte secret = 64 hex chars; block rewards go to
the address derived from it. The miner must use the **same** `IRIUM_RPC_TOKEN` as your
node. Expect log lines like `submitted all-gates block height=N`.

## 8. Troubleshooting

- **`Failed to load anchors`** → your working directory has no `bootstrap\anchors.json`.
  Extract `bootstrap.tar.gz` there, or run from the repo root (source build).
- **`peer_count` stays 0** → 38401 inbound blocked/not forwarded, or seeds unreachable.
  `Test-NetConnection 157.173.116.134 -Port 38401`.
- **Height stuck far behind the seeds** → stop the node, delete `data\blocks`,
  `data\state`, `data\candidate_admissions.dat`, and restart to re-sync from genesis
  (known testnet sync limitation we are tracking).
- **Miner `HTTP 400/403`** → node and miner env gate values differ, or the
  `IRIUM_RPC_TOKEN` differs between them. Use the unchanged `testnet.env` for both and
  the same token.

## 9. Report results

Fill in `TESTNET_FEEDBACK.md` and open a GitHub issue on `iriumlabs/irium` titled
`[testnet] <your node name>`.
