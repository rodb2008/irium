# Maintenance Plan: VPS iriumd Stale Binary Restart

## What the Stale-Binary Condition Is

The running `iriumd` process on `irium-vps` was started from a binary that was
subsequently replaced on disk. The kernel shows the open file descriptor as `(deleted)`:

    /proc/<pid>/exe -> /home/irium/irium/target/release/iriumd (deleted)

- In-memory binary (running): SHA256 6116ddd9...
- On-disk binary (current):   SHA256 406ec224...

The running process continues using the old binary until it exits or is restarted.
Any Restart=on-failure triggered by a crash would pick up the new on-disk binary.

---

## Why This Is Not an Emergency

- The node is fully operational: 20 peers, local height = best height, sync ahead = 0.
- The in-memory binary is functional for the current chain state.
- The on-disk binary is the correct LWMA-aware build from irium-lwma-upgrade
  (commit 5f0c732, activation height 16462) -- the same source as the working EU node.
- The VPS miner (separate process) is unaffected and hashing normally at ~16.6 MH/s.
- No blocks have been rejected or missed due to this condition.
- A restart causes a ~30-second peer reconnect interruption, not a data risk.

Defer until a natural low-activity window or when a restart is otherwise needed
(e.g., config change, system update, or after observing a slow block period).

---

## Pre-Restart Checklist

Run these checks immediately before issuing the kill. Do not proceed if any check fails.

### 1. Confirm on-disk binary is intact and correct

    ssh irium-vps 'sha256sum ~/irium/target/release/iriumd'
    # Expected: 406ec224... (the irium-lwma-upgrade build)

    ssh irium-vps 'ls -lh ~/irium/target/release/iriumd'
    # Must exist and not be a broken symlink

### 2. Confirm on-disk binary loads genesis correctly

    ssh irium-vps '~/irium/target/release/iriumd --help 2>&1 | head -5'
    # Must print usage/help, must NOT print "load locked genesis: No such file"
    # If it panics, do NOT proceed -- rebuild from irium-lwma-upgrade first

### 3. Confirm miner service is running and healthy

    ssh irium-vps 'systemctl is-active irium-miner.service'
    # Must return: active

### 4. Record current chain tip before restart

    ssh irium-vps 'journalctl -u iriumd.service -n 3 --no-pager | grep heartbeat'
    # Note: best height=XXXX, tip=YYYYYY -- baseline for post-restart check

### 5. Confirm no rogue miner processes

    ssh irium-vps 'ps aux | grep irium-miner | grep -v grep'
    # Must show exactly ONE irium-miner process (lesson from 2026-04-06 incident)

---

## Restart Procedure

The node runs as the irium user (no passwordless sudo). Use kill directly;
Restart=on-failure in the systemd unit will bring it back on the new binary.

    # Step 1: Get current PID
    ssh irium-vps 'pgrep -a iriumd'

    # Step 2: Kill (SIGKILL triggers Restart=on-failure)
    ssh irium-vps 'kill -9 $(pgrep iriumd)'

    # Step 3: Wait for restart
    sleep 15

    # Step 4: Confirm new process is running
    ssh irium-vps 'pgrep -a iriumd'

    # Step 5: Confirm new binary is NOT deleted
    ssh irium-vps 'readlink /proc/$(pgrep iriumd)/exe'
    # Must NOT contain the word "deleted"

---

## Rollback Steps

### Scenario A: Binary panics on genesis load

Symptom: journalctl shows "load locked genesis: No such file"

The on-disk binary has a stale CARGO_MANIFEST_DIR embedded. Rebuild from source:

    ssh irium-vps 'cd ~/irium-lwma-upgrade && cargo build --release 2>&1 | tail -5'
    ssh irium-vps 'cp ~/irium-lwma-upgrade/target/release/iriumd \
                      ~/irium/target/release/iriumd.tmp && \
                   mv ~/irium/target/release/iriumd.tmp \
                      ~/irium/target/release/iriumd'
    ssh irium-vps 'kill -9 $(pgrep iriumd)'

### Scenario B: Miner loses RPC connection and does not self-recover within 60s

    ssh irium-vps 'kill -9 $(pgrep irium-miner)'
    # systemd Restart=on-failure will restart it

### Scenario C: Node starts but peers=0 for more than 2 minutes

This is normal transient churn -- wait 5 minutes. If still 0:

    ssh irium-vps 'systemctl cat iriumd.service | grep ExecStart'
    # Verify it points to the correct binary path

---

## Post-Restart Verification

Run within 5 minutes of the restart:

    # 1. Binary path is not deleted
    ssh irium-vps 'readlink /proc/$(pgrep iriumd)/exe'

    # 2. Heartbeat shows local height >= pre-restart height, peers > 0
    ssh irium-vps 'journalctl -u iriumd.service -n 10 --no-pager | grep heartbeat'

    # 3. Miner caught up and mining correct next height
    ssh irium-vps 'journalctl -u irium-miner.service -n 5 --no-pager'

    # 4. Exactly one miner process running
    ssh irium-vps 'ps aux | grep irium-miner | grep -v grep'

    # 5. Load average reasonable within 2 minutes
    ssh irium-vps 'uptime'

---

## Notes

- The iriumd service ExecStart is in the base unit (no drop-in override).
  Verify before restart: systemctl cat iriumd.service | grep ExecStart
- The miner drop-in zzzzz-source-of-truth.conf controls the miner ExecStart.
  The miner binary path does NOT change during this procedure.
- NEVER run irium-miner --version as a health check -- the binary ignores the flag
  and begins mining. Use systemctl is-active or pgrep instead.
