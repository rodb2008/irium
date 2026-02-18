# Irium Public Stratum Pool

## Quickstart (miners)
Use the public pool endpoint:

- URL: `stratum+tcp://pool.iriumlabs.org:3333`
- Username: `IRM_ADDRESS.worker1`
- Password: `x`

Example username:
- `Q8Ni6TJ6Y77vvtMZ1E474kn2jYNawjvaLa.worker1`

## Payout model (SOLO)
This pool runs in SOLO mode.

- Your username must start with your IRM address.
- If your share finds a valid network block, the block reward is paid to that IRM address.
- Worker suffix (for example `.worker1`) is only for rig identification.

## Connectivity troubleshooting
If you cannot connect:

1. Check DNS resolution:
```bash
dig pool.iriumlabs.org +short
```

2. Check TCP reachability on port 3333:
```bash
nc -vz pool.iriumlabs.org 3333
```

3. If DNS resolves but connect fails:
- Confirm outbound TCP/3333 is allowed on your firewall/router.
- Check ISP/VPS filtering for custom mining ports.
- Try from another network to rule out local ISP filtering.

## How to verify service is up
From any Linux/macOS shell:
```bash
nc -vz pool.iriumlabs.org 3333
```

From the pool host:
```bash
systemctl status irium-stratum --no-pager
ss -lntp | egrep ':3333'
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
