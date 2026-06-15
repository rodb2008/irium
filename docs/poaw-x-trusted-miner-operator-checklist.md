# PoAW-X Trusted Miner Pilot — Operator Checklist

**Version:** 1.0 (post Phase 14-F)
**Use:** Work top-to-bottom before invite, during, and at shutdown. All commands are sanitized — no tokens/auth/IPs. Replace `<DEVNET_PID>` with the running testnet node PID.

> Mainnet must never be touched. RPC 39511 / status 39508 stay private throughout.

---

## A. Branch / hash verification
```bash
cd /home/irium/irium
git rev-parse --abbrev-ref HEAD        # testnet/poawx-phase12-completion-rc-hardening
git rev-parse --short HEAD             # a0aedc6
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
```bash
for p in 39510 39511 39508 39512; do ss -ltn | grep -q ":$p " && echo "$p listening" || echo "$p down"; done
```
- [ ] 39512 (stratum) listening; 39510/39511/39508 as expected.

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
- [ ] Only `STRATUM_PORT` (39512) reachable from the miner; 39511/39508 blocked publicly.

## G. Stratum verification
```bash
ss -ltn | grep ':39512'                 # stratum listening
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
```bash
# devnet only — never mainnet ports
for p in 39512 39510 39511 39508; do fuser -k $p/tcp 2>/dev/null; done
for p in 39510 39511 39508 39512; do ss -ltn | grep -q ":$p " && echo "$p STILL UP" || echo "$p clear"; done
```
- [ ] Devnet stratum + node stopped; ports clear.
- [ ] Both mainnets still on official binary, PIDs unchanged.
- [ ] (optional) devnet data dirs under `$HOME` removed.
