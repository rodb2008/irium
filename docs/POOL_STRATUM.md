# Irium Public Stratum Pool

## Quickstart (miners)
Primary pool endpoint:

- URL: `stratum+tcp://pool.iriumlabs.org:3333`
- Fallback URL: `stratum+tcp://157.173.116.134:3333`
- Username: `IRM_ADDRESS.worker1`
- Password: `x`

Example username:
- `Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa.worker1`

## Recommended failover config
Use 3 pool entries in miner UI/config:

1. `stratum+tcp://pool.iriumlabs.org:3333`
2. `stratum+tcp://157.173.116.134:3333`
3. `stratum+tcp://157.173.116.134:3333`

This keeps miners online if DNS resolution fails locally.

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

2. Check TCP reachability on port 3333 (hostname + direct IP):
```bash
nc -vz pool.iriumlabs.org 3333
nc -vz 157.173.116.134 3333
```

3. Test a raw Stratum subscribe:
```bash
printf '{"id":1,"method":"mining.subscribe","params":[]}\n' | nc 157.173.116.134 3333
```

4. If hostname fails but IP works:
- DNS issue on miner network/ISP.
- Keep mining on direct IP and fix resolver settings later.

5. If both fail:
- Confirm outbound TCP/3333 is allowed on firewall/router.
- Check ISP/VPS filtering for mining ports.
- Try another network to isolate local filtering.

## How to verify service is up
From any Linux/macOS shell:
```bash
nc -vz pool.iriumlabs.org 3333
nc -vz 157.173.116.134 3333
```

From the pool host:
```bash
systemctl status irium-stratum --no-pager
ss -lntp | egrep ':3333|:8081|:8332'
```

## Operator notes

### Required ports
- Public: `3333/tcp` (Stratum)
- Node P2P: `38291/tcp` (if this host also runs a node)
- Keep node RPC private: `127.0.0.1:38300` only

### Service control
```bash
sudo systemctl restart irium-stratum
sudo systemctl status irium-stratum --no-pager
journalctl -u irium-stratum -f
```

### Config locations
- Stratum env: `/etc/irium-pool/stratum.env`
- Service unit: `/etc/systemd/system/irium-stratum.service`
- Source: `/opt/irium-pool/irium-stratum`

### Security requirements
- Keep `/etc/irium-pool/stratum.env` mode `600`.
- Do not expose `127.0.0.1:38300` publicly.
- Keep `IRIUM_RPC_TOKEN` secret.

## Common log messages
- `[tmpl] fetch failed ... operation timed out`
: Temporary local RPC/connect stall. Stratum retries automatically.

- `[conn] bad json ...`
: Non-Stratum or malformed traffic hit port 3333.

- `[share] reject ... reason=low_difficulty`
: Share did not meet current pool difficulty target.

- `[block] submitted ...`
: A block candidate met network target and was submitted to the node.

## Disclaimer
Current implementation has been validated with local Stratum handshake and submit-path testing. Broader validation with real ASIC miners is welcome.

## Legacy cgminer compatibility (March 2, 2026)
A server-side compatibility update was deployed for older ASIC clients (including older cgminer/bmminer variants).

If direct IP TCP connects but your miner still reports "No servers were found", verify:
- Algorithm is SHA-256/SHA-256d
- SSL/TLS is disabled (`stratum+tcp://`)
- Worker format is `IRM_ADDRESS.worker1`

Use the 3-pool failover config above and share timestamped logs for handshake-level troubleshooting.
