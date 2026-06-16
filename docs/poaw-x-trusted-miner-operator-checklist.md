# PoAW-X Trusted Miner Pilot — Operator Checklist

**Version:** 2.0 (post Phase 15 — native_rewardable route proven)
**Use:** Work top-to-bottom before invite, during, and at shutdown. All commands are sanitized — no tokens/auth/IPs. Replace `<DEVNET_PID>` with the running testnet node PID.

> Mainnet must never be touched. RPC 39511 / status 39508 stay private throughout.

---

## A. Branch / hash verification
```bash
cd /home/irium/irium
git rev-parse --abbrev-ref HEAD        # native_rewardable code at testnet/poawx-phase13-native-rewardable-cpuminer-e2e
git rev-parse --short HEAD             # fad21c4 (native_rewardable route)
git status --porcelain                 # empty = clean
git log --show-signature -1 | grep -i "Good .*signature"   # signed
```

## B. Mainnet safety verification (both hosts)
```bash
# VPS-1
systemctl show iriumd.service -p ExecStart | grep -o 'path=[^ ;]*'   # /home/irium/mainnet/bin/iriumd-current
P=$(for p in /proc/[0-9]*; do case "$(readlink $p/exe 2>/dev/null)" in */mainnet/bin/iriumd-*) basename $p;; esac; done | head -1)
sha256sum /proc/$P/exe                 # 7c07ae2c... (official)
# irium-eu (same checks via: ssh irium-eu '...')
```
- [ ] Both mainnet PIDs unchanged; official hash; height advancing.
- [ ] No `IRIUM_POAWX_MODE` in either mainnet service env.

## C. Testnet process verification
```bash
# the testnet node runs with IRIUM_NETWORK=devnet, PoAW-X active
ps -o pid,etime,cmd -p <DEVNET_PID>
for p in /proc/[0-9]*; do case "$(readlink $p/exe 2>/dev/null)" in *iriumd*) :;; esac; done   # devnet binary != mainnet path
```
- [ ] Devnet node running from repo target / isolated devnet path, NOT the mainnet service binary.

## D. Port verification
> Ports below are EXAMPLES. Use the operator-selected Phase-N ports for this pilot
> (internal/devnet: Node RPC/status/P2P on 127.0.0.1 or VPS-restricted; external: the
> operator-selected stratum port, source-restricted to the miner IP). 39512 is optional,
> NOT mandatory.
```bash
for p in <NODE_P2P> <NODE_RPC> <NODE_STATUS> <STRATUM_PORT>; do ss -ltn | grep -q ":$p " && echo "$p listening" || echo "$p down"; done
```
- [ ] The operator-selected <STRATUM_PORT> (stratum) + node RPC/status/P2P listening as expected (ports are per-pilot; 39512 not mandatory).

## E. RPC-private verification
```bash
# from the server (localhost) RPC responds; from public it must be refused
ss -ltn | grep ':39511'                 # bound to 127.0.0.1 only
# externally (or via public IP) the connection must be REFUSED — verify from off-host
```
- [ ] RPC 39511 bound to localhost; refused from public IP.

## F. Firewall verification
```bash
sudo ufw status 2>/dev/null || sudo iptables -S 2>/dev/null   # operator-run (sudo)
```
- [ ] Only the operator-selected `STRATUM_PORT` reachable, **source-restricted to the miner IP** (never Anywhere); RPC/status private on 127.0.0.1.

## G. Stratum verification
```bash
ss -ltn | grep ':<STRATUM_PORT>'                 # stratum listening
journalctl -u <testnet-stratum-unit> --no-pager -n 30 | sed -E 's/[0-9]{1,3}(\.[0-9]{1,3}){3}/<ip>/g'
```
- [ ] Testnet stratum up and pointed at the testnet node.

## H. Receipt verification (private RPC, localhost)
```bash
# pending receipts present after assignment/submission (localhost RPC; auth header NOT shown here)
curl -s http://127.0.0.1:39511/poawx/pending  | head    # count > 0 when receipts pending
```
- [ ] Receipt persists in pending pool; clears after block commit.

## I. irx1_root verification (private RPC)
```bash
curl -s "http://127.0.0.1:39511/rpc/block?height=<N>" | grep -o '"irx1_root":"[0-9a-f]*"'   # non-null on receipt block
```
- [ ] Block JSON exposes a non-zero `irx1_root` for receipt-bearing blocks.

## J. P2P sync verification (if a second testnet peer is used)
```bash
journalctl -u <testnet-unit> --no-pager -n 40 | grep -iE "synced|heartbeat" | tail -2 | sed -E 's/[0-9]{1,3}(\.[0-9]{1,3}){3}/<ip>/g'
```
- [ ] Peer reaches same height + same tip hash.

## K. Log capture checklist
- [ ] Capture testnet node log for the session window.
- [ ] Capture testnet stratum log.
- [ ] **Sanitize** before sharing: mask IPs, strip any auth/token lines.

## L. Miner result capture checklist
- [ ] Received miner report (subscribe/authorize/notify/accepted/rejected/duration).
- [ ] Note first-accepted-share timestamp and any disconnects.

## M. Shutdown checklist

> SAFETY (ref: docs/poaw-x-mainnet-cleanup-incident.md): NEVER use `pkill -f "iriumd"`
> or any bare `iriumd`/`irium` process-name match — it kills the production node.
> Teardown only by exact pidfile or exact devnet port. Record pilot PIDs at startup.
> Verify the production MainPID + hash are UNCHANGED before and after teardown.

```bash
# record at startup:  echo $! > /tmp/pilot-node.pid ; echo $! > /tmp/pilot-stratum.pid
# verify prod BEFORE teardown:
PROD_PID_BEFORE=$(systemctl show -p MainPID --value iriumd)
# teardown by exact pidfile (preferred):
kill "$(cat /tmp/pilot-stratum.pid)" 2>/dev/null; kill "$(cat /tmp/pilot-node.pid)" 2>/dev/null
# or by exact devnet port (never a mainnet port):
for p in 39512 39510 39511 39508; do fuser -k $p/tcp 2>/dev/null; done
for p in 39510 39511 39508 39512; do ss -ltn | grep -q ":$p " && echo "$p STILL UP" || echo "$p clear"; done
```
- [ ] Devnet stratum + node stopped; ports clear.
- [ ] Production MainPID AFTER == PROD_PID_BEFORE (unchanged): `[ "$(systemctl show -p MainPID --value iriumd)" = "$PROD_PID_BEFORE" ] && echo OK`.
- [ ] `systemctl is-active iriumd` == active; running exe hash still `7c07ae2c…`.
- [ ] Both mainnets still on official binary, PIDs unchanged.
- [ ] (optional) devnet data dirs under `$HOME` removed (exact paths only).


## N. Native_rewardable route validation (PoAW-X rewardable path)

> Rewardable blocks come ONLY from the gated `native_rewardable` route. `cpuminer_compat`
> stays NON-rewardable on PoAW-X (share accounting only); no variant sweep promotes a block.

Stratum env for the pilot (Phase 13 gated route):
```
IRIUM_NETWORK=devnet IRIUM_STRATUM_ADAPTER_MODE=auto IRIUM_STRATUM_NATIVE_REWARDABLE_ENABLED=1
IRIUM_STRATUM_POAWX=1 IRIUM_STRATUM_MINER_FAMILY=cpuminer STRATUM_DEFAULT_DIFF=0.001
STRATUM_CARRIERS=off IRIUM_POAWX_MODE=active
```
- [ ] Stratum logs `adapter_kind=native_rewardable_reserved` and `sub-1 difficulty floor active`.
- [ ] Miner connects from the **expected miner IP** (stratum source-restricted).
- [ ] `REWARDABLE_SHARE_ACCEPTED` -> `REWARDABLE_CANDIDATE` -> `submit_block_extended` -> `BLOCK_ACCEPTED`.
- [ ] Node A `block_extended accepted ... cleared_receipts=N`; block `irx1_root` non-zero and == seeded receipt root.
- [ ] Node B reaches the SAME height + tip hash via P2P.
- [ ] Payout/worker address in stratum logs == the supplied miner address.

## O. Temporary firewall (source-restricted) add/remove

> Operator runs sudo; agent only prints the commands. Never `Anywhere`. Require the miner IP first.
```
# add (VPS-1):
sudo ufw allow from <VPS-2-IP> to any port <NODE_A_P2P>  proto tcp
sudo ufw allow from <MINER_IP> to any port <STRATUM_PORT> proto tcp
# remove immediately after the pilot + verify absent:
sudo ufw delete allow from <VPS-2-IP> to any port <NODE_A_P2P>  proto tcp
sudo ufw delete allow from <MINER_IP> to any port <STRATUM_PORT> proto tcp
sudo ufw status verbose | grep -E "<NODE_A_P2P>|<STRATUM_PORT>" || echo rules-absent
```
- [ ] Both rules source-restricted (NOT Anywhere), added before mining.
- [ ] Both rules removed + verified absent after the pilot.
