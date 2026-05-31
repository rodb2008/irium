# Irium Stratum Mining Setup Guide

This is the one-stop guide for getting any mining hardware — ASIC, GPU,
or CPU — hashing on Irium. It covers both pool mining (the default for
almost everyone) and solo mining (running against your own node). All
commands, URLs, ports, and env-var names are exact and current as of
the present chain state.

If you only have five minutes, jump straight to the section that matches
your hardware. Everything you need to copy-paste is in plain text blocks.

---

## Quick pick

| Your hardware / situation                          | Read this section |
|----------------------------------------------------|-------------------|
| Bitaxe, Antminer S19/S21, Whatsminer, Avalon       | 1. Pool mining for ASIC miners |
| NVIDIA or AMD graphics card                        | 2. Pool mining for GPU miners |
| Just a regular computer CPU                        | 3. Pool mining for CPU miners |
| ASIC + my own iriumd node (no pool)                | 4. Solo mining for ASIC miners |
| GPU + my own iriumd node (no pool)                 | 5. Solo mining for GPU miners |
| CPU + my own iriumd node (no pool)                 | 6. Solo mining for CPU miners |
| I rented hashrate on NiceHash or MiningRigRentals  | 7. MRR and NiceHash rental config |
| Something is not working                           | 8. Troubleshooting |
| I want to confirm shares/blocks are real           | 9. Pool verification guide |

---

## Plain-English glossary

These terms appear throughout the guide.

| Term            | What it means                                                                                                                              |
|-----------------|--------------------------------------------------------------------------------------------------------------------------------------------|
| **Stratum**     | The protocol mining hardware uses to talk to a pool. It runs over plain TCP. Not encrypted.                                                |
| **ASIC**        | Application-Specific Integrated Circuit. Dedicated mining hardware like Bitaxe, Antminer, Whatsminer, Avalon. Plug in, set pool URL, hash. |
| **GPU mining**  | Mining software (T-Rex, lolMiner, NBMiner, ccminer) running on a graphics card. Slower than ASIC, faster than CPU.                          |
| **CPU mining**  | Mining software running on your computer's main processor. Slow. Mostly useful when the CPU is otherwise idle.                              |
| **Pool**        | A server that combines many small miners' work and smooths out payouts. Without a pool, payouts are very lumpy.                            |
| **Solo**        | Mining against your own node. You get the full block reward when you find one, or nothing when you don't.                                  |
| **SHA-256d**    | The hash algorithm Irium uses. Same as Bitcoin. Every SHA-256d miner in existence can mine Irium.                                          |
| **Worker name** | What you put in the Stratum `User` field. On Irium pools it must start with your IRM address (the wallet where rewards go).                |
| **Vardiff**     | The pool keeps adjusting how hard each share is for you so you submit about 4 shares per minute, no matter how fast your hardware is.       |
| **Share**       | A near-solution. Most shares do not find blocks — they are proof you are working. Some shares do find blocks and earn the reward.            |
| **Block reward**| 50 IRM per block right now. Halves every 210,000 blocks.                                                                                   |
| **Coinbase maturity** | A newly mined coinbase output is locked for 100 blocks (~3 hours at the chain's current 1-2 minute block rate) before you can spend it.        |
| **IRM address** | Starts with `P` or `Q` (e.g. `Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa`). Generate one with the Irium Core desktop app or `irium-wallet new-address`. |

---

## The official pool at a glance

You need this information almost everywhere below.

| Detail              | Value                                                                                       |
|---------------------|---------------------------------------------------------------------------------------------|
| Pool hostname       | `pool.iriumlabs.org`                                                                        |
| ASIC port           | `3333`                                                                                      |
| CPU/GPU port        | `3335`                                                                                      |
| ISP-block fallback  | `443` (HTTPS port — same Stratum bytes, just routed through a port DPI cannot easily block) |
| Second fallback     | `80` (HTTP port)                                                                            |
| Stats page (humans) | `https://pool.iriumlabs.org/stats`                                                          |
| Stats JSON (scripts)| `http://pool.iriumlabs.org:3337/stats`                                                       |
| Algorithm           | SHA-256d                                                                                    |
| Protocol            | Stratum v1 only. TLS off. Connect with `stratum+tcp://` (not `stratum+ssl://`).             |
| Worker name format  | `IRM_ADDRESS.LABEL` (the address is where rewards go; the label is for your own bookkeeping)|
| Password            | `x` (any non-empty string works)                                                            |

Worker-name example:

```
Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa.kitchen-rig
```

---

## Section 1: Pool mining for ASIC miners

**Use port 3333.** This is the strict canonical Stratum profile, tuned
for high-hashrate hardware. The default starting difficulty is 10,000
and the pool's vardiff cap is 2,000,000 — high enough for any
single-board ASIC up through full-size farm units.

### 1a. Bitaxe (ESP-Miner / AxeOS)

Open the AxeOS web UI on your local network (usually
`http://bitaxe.local`), go to **Settings → Stratum Pool**, and fill in:

| Field                | Value                                       |
|----------------------|---------------------------------------------|
| Stratum URL          | `pool.iriumlabs.org`                        |
| Stratum Port         | `3333`                                      |
| Stratum User         | `<YOUR_IRM_ADDRESS>.bitaxe1`                |
| Stratum Password     | `x`                                         |
| Algorithm            | SHA-256 / SHA-256d                          |
| TLS / Stratum SSL    | OFF                                         |

Save, reboot the Bitaxe. Within about 30 seconds the AxeOS web UI
should show "Connected" and shares should start scrolling. Look at
`https://pool.iriumlabs.org/stats` and search for your worker — you
should see it appear within a minute.

If port 3333 does not work (you see "connection refused" or "timeout"),
switch the port to `443`. The Stratum protocol is the same; the
HTTPS-style port just slips past ISP filters that block 3333.

### 1b. Antminer S19 / S21 series, Whatsminer, Avalon

In your miner's web UI, open **Configuration → Pool 1** (or whichever
slot you want to use) and fill in:

| Field    | Value                                          |
|----------|------------------------------------------------|
| URL      | `stratum+tcp://pool.iriumlabs.org:3333`        |
| Worker   | `<YOUR_IRM_ADDRESS>.<your-label-e.g.-rig1>`    |
| Password | `x`                                            |

Apply, the miner will reconnect automatically. If your firmware
prefers a backup pool, put `stratum+tcp://pool.iriumlabs.org:443`
in Pool 2 so a 3333-block falls back to 443 automatically.

### 1c. What to expect when it is working

- "Connected" indicator on the miner UI within a few seconds.
- First share within ~30 seconds (a Bitaxe at 700 GH/s will produce a
  share roughly every 7 seconds at the starting difficulty; bigger
  ASICs will see vardiff climb fast in the first minute and settle).
- Share accept rate above 95%.
- Your worker appears on `https://pool.iriumlabs.org/stats` within
  about a minute.

### 1d. Block 22,888 fork — already activated

The chain switched to Bitcoin-standard block-header serialization at
block 22,888 (Fix 2a). The chain is well past this height now. You do
not need to do anything special — any standard SHA-256d ASIC produces
valid Irium blocks today.

---

## Section 2: Pool mining for GPU miners

**Use port 3335.** This is the legacy compatibility profile, tuned for
lower-hashrate hardware. The default starting difficulty is 1.

### 2a. T-Rex (NVIDIA)

```
t-rex -a sha256d \
      -o stratum+tcp://pool.iriumlabs.org:3335 \
      -u <YOUR_IRM_ADDRESS>.gpu1 \
      -p x
```

### 2b. lolMiner (NVIDIA + AMD)

```
lolMiner --algo SHA256D \
         --pool stratum+tcp://pool.iriumlabs.org:3335 \
         --user <YOUR_IRM_ADDRESS>.gpu1 \
         --pass x
```

### 2c. NBMiner

```
nbminer -a sha256d \
        -o stratum+tcp://pool.iriumlabs.org:3335 \
        -u <YOUR_IRM_ADDRESS>.gpu1 \
        -p x
```

### 2d. ccminer (older CUDA)

```
ccminer -a sha256d \
        -o stratum+tcp://pool.iriumlabs.org:3335 \
        -u <YOUR_IRM_ADDRESS>.gpu1 \
        -p x
```

### 2e. Bundled `irium-miner-gpu`

The repository's own GPU miner. OpenCL — works on NVIDIA, AMD, Intel.

```
./target/release/irium-miner-gpu \
  --pool   stratum+tcp://pool.iriumlabs.org:3335 \
  --wallet <YOUR_IRM_ADDRESS>
```

Multi-GPU is automatic. To pin to specific cards:

```
./target/release/irium-miner-gpu \
  --pool   stratum+tcp://pool.iriumlabs.org:3335 \
  --wallet <YOUR_IRM_ADDRESS> \
  --devices 0,1,3
```

`--list-platforms` enumerates all detected OpenCL devices first.

### 2f. What to expect when it is working

- One-line "Connected" message in the miner's log.
- Shares start within a few seconds.
- Hashrate display stabilises after a minute or two.
- Your worker appears on `https://pool.iriumlabs.org/stats`.

---

## Section 3: Pool mining for CPU miners

**Use port 3335.** Same profile as GPU miners.

### 3a. cpuminer-opt (most widely used)

```
cpuminer-opt -a sha256d \
             -o stratum+tcp://pool.iriumlabs.org:3335 \
             -u <YOUR_IRM_ADDRESS>.cpu1 \
             -p x \
             -t <NUMBER_OF_CPU_THREADS>
```

Replace `<NUMBER_OF_CPU_THREADS>` with how many CPU threads you want
to use. Leave one or two threads free if you also use the machine
for anything else.

### 3b. cpuminer-multi / sgminer-gm

Same flags as cpuminer-opt above, same port and URL.

### 3c. Bundled `irium-miner` (this repo) is NOT for pool mining

The repository's own `irium-miner` CPU binary only does **solo** mining
against a local iriumd. There is no `--pool` flag on it. If you want
pool CPU mining, install cpuminer-opt separately as shown above.

### 3d. What to expect when it is working

- "stratum subscribe ok" line in the cpuminer log.
- First share in 10–60 seconds depending on CPU speed.
- "yes!" or "accepted" lines from then on.
- Share accept rate above 95%.

---

## Section 4: Solo mining for ASIC miners

Solo means: no pool, no operator, no shared payouts. You run your
own Irium node, an ASIC submits work to it, and any block your ASIC
finds pays the full 50 IRM directly to you.

Solo is reasonable on a small chain like Irium today. On a busy
chain it would be impractical for a single ASIC. Pool mining still
gives smoother payouts.

### 4a. Architecture

```
ASIC miner
   |
   |  stratum+tcp://your-machine:3333
   v
irium-miner --solo-stratum         (this is the user-side bridge)
   |
   |  HTTP /rpc/getblocktemplate, /rpc/submit_block
   v
iriumd RPC on 127.0.0.1:38300      (this is your own full node)
   |
   v
Irium P2P network
```

iriumd itself has no Stratum listener. The bridge lives in the
`irium-miner` binary under the `--solo-stratum` flag. The bridge
fetches block templates from iriumd, hands them to the ASIC as
Stratum jobs, and submits solved blocks back to iriumd over HTTP.

### 4b. Step-by-step

**Step 1: Start your own iriumd and wait for sync.**

```
./target/release/iriumd
```

Or, if you use the Irium Core desktop app, just open it — it launches
iriumd in the background and binds RPC on `127.0.0.1:38300`. Wait for
the height in the UI to match `https://pool.iriumlabs.org/stats`.

**Step 2: Set the RPC token if your iriumd has one.**

```
export IRIUM_RPC_TOKEN=<same-token-as-iriumd>
export IRIUM_NODE_RPC=http://127.0.0.1:38300
```

If you do not have a token configured on iriumd, skip this. The desktop
app sets a token automatically; if you run the bundled `irium-miner`
inside the desktop app's launcher, the token is wired up for you.

**Step 3: Start the solo Stratum bridge.**

```
./target/release/irium-miner --solo-stratum --listen 0.0.0.0:3333
```

The bridge now listens on TCP port 3333 (you can pick any free port).
Leave this terminal running.

Env-var equivalent (useful for systemd or scripts):

```
export IRIUM_SOLO_STRATUM=1
export IRIUM_SOLO_STRATUM_LISTEN=0.0.0.0:3333
export IRIUM_SOLO_STRATUM_DIFFICULTY=1
./target/release/irium-miner
```

Tuning env vars (defaults usually fine):

| Variable                            | Default     | Purpose                                                                |
|-------------------------------------|-------------|------------------------------------------------------------------------|
| `IRIUM_SOLO_STRATUM_EXTRANONCE2_SIZE` | `4`       | Bytes of extranonce2 advertised. Bump only if an ASIC needs a wider field. |
| `IRIUM_SOLO_STRATUM_REFRESH_SECS`     | built-in  | How often to poll iriumd for a new template. Lower = more responsive on tip change. |
| `IRIUM_SOLO_STRATUM_DIFFICULTY`       | built-in  | Initial pool difficulty advertised to the ASIC.                          |

**Step 4: Point the ASIC at your bridge.**

In the ASIC's web UI:

| Field          | Value                                                       |
|----------------|-------------------------------------------------------------|
| Stratum URL    | `stratum+tcp://<your-machine-LAN-or-VPS-IP>:3333`           |
| Worker         | `<YOUR_IRM_ADDRESS>.worker1`                                |
| Password       | `x`                                                         |
| TLS            | OFF                                                         |

If the worker username does not parse as a valid IRM address, the
bridge falls back to whatever `IRIUM_MINER_ADDRESS` is set to.

**Step 5: Verify.**

- The bridge prints `[solo-stratum] authorized <worker> payout <address>`.
- On finding a block, you see `[solo-stratum] submitted block height N hash X`.
- iriumd's journal shows `[block] accepted height=<N> hash=<HASH> miner=<address>`.
- After 100-block coinbase maturity (~3 hours), the 50 IRM is spendable.

### 4c. Why not point the ASIC directly at iriumd?

iriumd's RPC speaks HTTP/JSON, not Stratum wire. ASIC firmware speaks
Stratum. The `irium-miner --solo-stratum` bridge is the translator.
There is no way around it for ASIC hardware.

---

## Section 5: Solo mining for GPU miners

GPUs do not need the bridge — the bundled GPU miner talks directly to
iriumd over HTTP.

**Step 1: Start your own iriumd and wait for sync.**

```
./target/release/iriumd
```

**Step 2: Run the GPU miner pointed at iriumd.**

```
./target/release/irium-miner-gpu \
  --rpc    http://127.0.0.1:38300 \
  --wallet <YOUR_IRM_ADDRESS>
```

Replace `127.0.0.1` with your iriumd machine's IP if the miner runs on
a different box. The RPC port must be reachable from the miner.

Multi-GPU is automatic. To pin:

```
./target/release/irium-miner-gpu \
  --rpc    http://127.0.0.1:38300 \
  --wallet <YOUR_IRM_ADDRESS> \
  --devices 0,1,3
```

If your iriumd requires the RPC token, export it before starting the
miner:

```
export IRIUM_RPC_TOKEN=<same-token-as-iriumd>
```

### Alternative: third-party SHA-256d miner through the solo bridge

T-Rex, lolMiner, NBMiner, ccminer can also use the solo bridge from
Section 4 instead of talking to iriumd directly. Setup is identical to
Section 4 but with the third-party miner in place of the ASIC. The
bridge does not care whether the upstream is silicon or software.

---

## Section 6: Solo mining for CPU miners

The bundled `irium-miner` CPU binary talks directly to iriumd over
HTTP. No bridge required.

**Step 1: Start your own iriumd and wait for sync.**

```
./target/release/iriumd
```

**Step 2: Run the CPU miner.**

```
export IRIUM_MINER_ADDRESS=<YOUR_IRM_ADDRESS>
export IRIUM_NODE_RPC=http://127.0.0.1:38300
./target/release/irium-miner
```

Optional: limit the number of CPU threads:

```
export IRIUM_MINER_THREADS=4
./target/release/irium-miner
```

If your iriumd requires the RPC token:

```
export IRIUM_RPC_TOKEN=<same-token-as-iriumd>
```

### Alternative: cpuminer-opt against the solo bridge

cpuminer-opt cannot speak iriumd's HTTP RPC directly. To use cpuminer-opt
in solo mode, run the `--solo-stratum` bridge as in Section 4 and point
cpuminer-opt at it as if it were a pool:

```
cpuminer-opt -a sha256d \
             -o stratum+tcp://127.0.0.1:3333 \
             -u <YOUR_IRM_ADDRESS>.cpu1 \
             -p x \
             -t <NUMBER_OF_CPU_THREADS>
```

---

## Section 7: MRR and NiceHash rental configuration

Renting hashrate from MiningRigRentals or NiceHash means you pay rentees
to point their hardware at the pool URL you provide. The rentee's
hardware then mines Irium under your IRM address for the duration of
the rental.

The official pool's port 3333 has been configured specifically for
rental compatibility — vardiff max is set to 2,000,000 so even
multi-TH/s rentals do not get pinned at the default starting diff and
stale-share themselves.

### 7a. NiceHash

NiceHash uses the algorithm name **"SHA256"** in its UI (in NiceHash
naming, "SHA256" means SHA-256d / double-SHA-256, which is what Irium
uses; NiceHash does not have a separate "SHA256d" option).

Create a rental with these fields:

| NiceHash field             | Value                                       |
|----------------------------|---------------------------------------------|
| Algorithm                  | SHA256                                      |
| Pool host                  | `pool.iriumlabs.org`                        |
| Pool port                  | `3333`                                      |
| Username                   | `<YOUR_IRM_ADDRESS>.nicehash1`              |
| Password                   | `x`                                         |
| Pool type                  | Stratum (TCP). Not Stratum+SSL.             |

For the **backup pool** field (NiceHash always wants one), use:

| Backup field   | Value                                       |
|----------------|---------------------------------------------|
| Host           | `pool.iriumlabs.org`                        |
| Port           | `443`                                       |
| User / pass    | Same as primary                             |

This way, if any rentee's network blocks 3333, NiceHash falls back to
443 automatically.

### 7b. MRR (MiningRigRentals)

MRR uses **"SHA-256"** (with a hyphen) for the algorithm name in its
profile editor. As with NiceHash, MRR's "SHA-256" is SHA-256d.

Create a pool profile:

| MRR pool-profile field | Value                                       |
|------------------------|---------------------------------------------|
| Pool host              | `pool.iriumlabs.org`                        |
| Pool port              | `3333`                                      |
| Pool worker            | `<YOUR_IRM_ADDRESS>.mrr1`                   |
| Pool password          | `x`                                         |
| Algorithm              | SHA-256                                     |

Then attach the profile to your rig listings.

### 7c. Common rental gotchas

| Issue                                                                                | Cause                                                                                                                        | Fix                                                                                                  |
|--------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------|
| Shares stay at very high difficulty for hours and you see almost none                | Rented hashrate is enormous; vardiff is still climbing.                                                                       | Wait 1–2 minutes for vardiff to settle. The 2,000,000 cap is high enough that it will not hit ceiling. |
| Stale-share rate above 10%                                                           | Rentee's network is far from the pool host and round-trip latency is high.                                                    | Cancel and rent a region closer to the pool's host region.                                            |
| Rental "starts" but no shares ever appear on `pool.iriumlabs.org/stats`              | Wrong port profile (3335 instead of 3333), or worker name does not start with an IRM address.                                  | Double-check both. Port must be 3333 for rentals. Worker must be `<IRM_ADDRESS>.<label>`.            |
| Block found, no reward shown in wallet                                               | Coinbase maturity is 100 blocks (~3 hours at current chain rate).                                                              | Wait. Check `irium-wallet history <address>` for the unmatured coinbase.                              |
| NiceHash reports "pool rejected too many shares"                                      | Rare. Usually a rentee's firmware is misconfigured for SHA-256d.                                                                | Switch to a different rentee or extend rental duration so the misbehaving one rotates out.            |

---

## Section 8: Troubleshooting common issues

### 8a. Quick symptom → cause → fix table

| Symptom                                                          | Cause                                                | Fix                                                                                                |
|------------------------------------------------------------------|------------------------------------------------------|----------------------------------------------------------------------------------------------------|
| Miner connects but submits zero shares                           | Worker name missing the IRM address                  | Set worker to `<YOUR_IRM_ADDRESS>.<label>`                                                          |
| "Connection refused" on port 3333                                | ISP blocks 3333                                      | Switch to port `443`                                                                               |
| Many `rejected_low_difficulty` lines                             | Hardware below port 3333's diff baseline             | Switch to port 3335                                                                                |
| Many `rejected_stale` lines                                      | High network latency or clock skew                   | Sync your clock with NTP. Check `ping pool.iriumlabs.org`. If > 200 ms, consider a closer location |
| Many `rejected_invalid` lines                                    | Wrong algorithm or hardware error                     | Confirm algorithm is `sha256d` / `SHA256D`. Check ASIC temperature                                  |
| Block found but balance still shows nothing                       | Coinbase maturity (100 blocks)                       | Wait ~3 hours. Use `irium-wallet history <address>` to see unmatured coinbase                       |
| `getent hosts pool.iriumlabs.org` fails                          | DNS                                                  | Switch DNS resolver (e.g. `8.8.8.8`); avoid hard-coded backend IPs                                  |
| Pool stats page does not show my worker                          | Worker still negotiating, or wrong port              | Wait 60 seconds. If still missing, recheck URL and port                                             |
| Solo bridge logs "401 unauthorized" from iriumd                  | Missing or wrong `IRIUM_RPC_TOKEN`                    | Export the same token as iriumd before starting the bridge                                          |
| Solo bridge logs "connection refused" to `127.0.0.1:38300`       | iriumd not running, or RPC bound to a different host  | Start iriumd. Check `IRIUM_NODE_RPC` matches iriumd's actual bind                                   |
| `t-rex` reports "stratum: protocol error, version-rolling refused" | T-Rex tried to negotiate version-rolling; pool refused | Harmless. T-Rex retries without it and AsicBoost is disabled. Mining continues normally               |

### 8b. Connectivity diagnostics

Test from the machine running the miner (Linux or macOS):

```
getent hosts pool.iriumlabs.org
nc -vz pool.iriumlabs.org 3333
nc -vz pool.iriumlabs.org 3335
nc -vz pool.iriumlabs.org 443
```

A successful test prints `succeeded` or similar. A failure means the
pool is unreachable from your network — either DNS, your firewall, or
your ISP. Try a different network to narrow it down.

Raw Stratum subscribe (advanced — confirms the port speaks Stratum):

```
printf '{"id":1,"method":"mining.subscribe","params":[]}\n' | nc pool.iriumlabs.org 3333
```

A working pool returns a JSON `result` array within a second. Anything
else (no response, HTML, "bad json") means the port is reachable but
not the right service.

### 8c. Solo mining specifics

Test iriumd RPC from the miner host:

```
curl http://127.0.0.1:38300/rpc/getblocktemplate
```

Expected: a JSON object with a `height` field. If you get a JSON error
about authentication, set `IRIUM_RPC_TOKEN`:

```
curl -H "Authorization: Bearer $IRIUM_RPC_TOKEN" \
     http://127.0.0.1:38300/rpc/getblocktemplate
```

If you get "connection refused", iriumd is not running on that host
or is not bound to that interface.

---

## Section 9: Pool verification guide

Use this section to confirm everything is working end-to-end before
walking away from the rig.

### 9a. Is the pool itself up?

From any Linux or macOS shell:

```
nc -vz pool.iriumlabs.org 3333
nc -vz pool.iriumlabs.org 3335
```

Both should print `succeeded`. If either fails, see Section 8b first.

You can also visit `https://pool.iriumlabs.org/stats` in any browser.
A blank page or 502 means the pool is down.

### 9b. Are my shares counting?

Open `https://pool.iriumlabs.org/stats` and search (Ctrl-F) for your
IRM address. Within about a minute of your miner connecting you should
see a row with:

- Your address
- A non-zero `accepted` counter that grows over time
- A small (ideally zero) `rejected` counter
- An `idle` time that stays small (under a couple of minutes; long idle
  means your miner stopped submitting)
- A `hashrate_15m` figure that roughly matches the hashrate your
  miner's own UI reports

For the raw JSON (useful for automation or scripts):

```
curl -s http://pool.iriumlabs.org:3337/stats | python3 -m json.tool | less
```

The structure includes per-profile (ASIC port 3333 and CPU/GPU port
3335) sections each with a `miners` array. Each `miners[]` entry has
the same fields as the human stats page.

### 9c. Did I find a block?

Two indicators:

1. **Pool stats page** `https://pool.iriumlabs.org/stats` has a
   `blocks_found` counter near the top and shows the most recent
   block-find events. If yours was the worker that found it, the
   block-find row will list your worker name.
2. **iriumd journal** (your own node, if you run one):
   ```
   [block] accepted height=<N> hash=<HASH> miner=<YOUR_IRM_ADDRESS>
   ```

### 9d. When and where do I get paid?

Every port (3333, 3335, 443) runs the **same direct-payout model**:
when one of your shares solves a block, the **full 50 IRM block
reward** goes directly to the IRM address in your worker name via the
coinbase output. **Zero pool fee.** The pool operator takes nothing
and the pool wallet does not accumulate or redistribute funds.

(The prior PPLNS share-window arrangement was removed after the
2026-05-29 over-distribution incident. There is no longer any pool-
wallet payout queue, no PPLNS share window, no `/payouts` endpoint,
and no `/miners_payout` endpoint.)

The reward becomes spendable after **100 blocks of coinbase maturity**.
At the current chain block rate of ~1–2 minutes per block, that is
about 3 hours after the block-find.

Check your balance once maturity has elapsed:

```
./target/release/irium-wallet balance <YOUR_IRM_ADDRESS>
```

Or use the Irium Core desktop app — the dashboard shows confirmed and
immature balances separately.

---

## Appendix A: One-page reference card

| What                               | Where                                                            |
|------------------------------------|------------------------------------------------------------------|
| Pool hostname                      | `pool.iriumlabs.org`                                             |
| ASIC pool port                     | `3333` (vardiff 1 → 2,000,000, default 10,000)                   |
| CPU/GPU pool port                  | `3335` (vardiff 1 → 4,096, default 1)                            |
| ISP-block fallback                 | `443` (then `80`)                                                |
| Stats (human)                      | `https://pool.iriumlabs.org/stats`                               |
| Stats (JSON)                       | `http://pool.iriumlabs.org:3337/stats`                           |
| Solo bridge binary                 | `irium-miner --solo-stratum --listen 0.0.0.0:3333`               |
| Solo GPU miner                     | `irium-miner-gpu --rpc http://127.0.0.1:38300 --wallet <addr>`   |
| Solo CPU miner (direct RPC)        | `IRIUM_MINER_ADDRESS=<addr> IRIUM_NODE_RPC=http://127.0.0.1:38300 irium-miner` |
| Node RPC port                      | `38300`                                                          |
| Node P2P port                      | `38291`                                                          |
| Node status (read-only)            | `http://127.0.0.1:8080/status`                                   |
| Algorithm                          | SHA-256d (= "SHA256" in NiceHash, "SHA-256" in MRR)              |
| Block reward                       | 50 IRM (halves every 210,000 blocks)                             |
| Coinbase maturity                  | 100 blocks (~3 hours at current block rate)                       |
| Worker name format                 | `<IRM_ADDRESS>.<label>`                                          |
| Password                           | `x`                                                              |
| TLS                                | Off — connect with `stratum+tcp://`                              |
| Fix 2a activation height           | 22,888 (already active)                                          |
| AuxPoW activation height           | 26,347 (not yet active)                                          |

---

## Appendix B: Where to go for more

| Topic                            | File                                            |
|----------------------------------|-------------------------------------------------|
| Operator-level pool internals    | `docs/POOL_STRATUM.md`                          |
| Solo Stratum bridge internals    | `docs/SOLO_STRATUM.md`                          |
| Bundled GPU miner detailed flags | `GPU-MINER.md`                                  |
| Merged mining (AuxPoW)           | `docs/MERGED-MINING.md`                         |
| One-click launcher scripts       | `mine-cpu.sh`, `mine-gpu.sh`, the `.bat` and `-mac.sh` variants |
| General mining overview          | `docs/MINING.md`                                |
| Desktop app one-click mining     | https://github.com/iriumlabs/irium-core/releases/latest          |
| GitHub issues / discussions      | https://github.com/iriumlabs/irium/issues       |
