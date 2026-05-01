# Running Irium with Docker

This guide covers running an Irium full node and miner using Docker.

## Quick start

```sh
# 1. Clone the repository
git clone https://github.com/iriumlabs/irium.git
cd irium

# 2. Copy and edit the environment file
cp .env.example .env
# Edit .env: set IRIUM_NODE_PUBLIC_IP to your server's public IP

# 3. Start the node
docker-compose up -d iriumd

# 4. Check it is syncing
docker-compose logs -f iriumd
```

The node will connect to the Irium seed network and begin syncing. Sync progress
appears in the logs as heartbeat lines showing height and peer count.

To also run the miner, first set `IRIUM_MINER_ADDRESS` in `.env`, then:

```sh
docker-compose up -d
```

---

## Configuration

All settings are passed as environment variables. The `.env.example` file lists
every configurable variable with a description and default value.

Copy it to `.env` before starting:

```sh
cp .env.example .env
```

### Variables you must set

| Variable | Description |
|---|---|
| `IRIUM_NODE_PUBLIC_IP` | Your server's public IP. Required if behind NAT so peers can connect back to you. |
| `IRIUM_MINER_ADDRESS` | IRM address for block rewards. Required only if running the miner. |

### Variables with defaults

| Variable | Default | Description |
|---|---|---|
| `IRIUM_P2P_EXTERNAL_PORT` | `38291` | Host port mapped to the P2P listener. The node always binds internally on 38291 — change this only if that port is already in use on your host. |
| `IRIUM_RPC_EXTERNAL_PORT` | `38300` | Host port mapped to the RPC/API endpoint |
| `IRIUM_STATUS_EXTERNAL_PORT` | `8080` | Host port mapped to the status endpoint (`GET /status`) |
| `IRIUM_MINER_THREADS` | `2` | CPU threads the miner uses |
| `IRIUM_RPC_TOKEN` | _(empty)_ | Shared secret for RPC authentication |

---

## Blockchain data persistence

The node stores all blockchain data in the named volume `irium_data`, mounted at
`/home/irium/.irium` inside the container. This volume persists across container
restarts, upgrades, and `docker-compose down` calls.

To list the volume:

```sh
docker volume ls | grep irium_data
```

To inspect its disk usage:

```sh
docker system df -v | grep irium_data
```

**Never delete this volume** unless you intend to re-sync from the network from scratch.

---

## Updating to a new release

```sh
# Pull the latest image
docker-compose pull iriumd irium-miner

# Restart with the new image (data volume is preserved)
docker-compose up -d
```

Or to rebuild from source:

```sh
docker-compose build --no-cache
docker-compose up -d
```

---

## Running irium-wallet commands against the node

The wallet CLI connects to the node's RPC endpoint. When the node is running in
Docker, point it at the mapped RPC port on localhost:

```sh
# Check balance (replace <address> with your IRM address)
irium-wallet balance --address <address> --rpc http://localhost:38300

# Create an offer
irium-wallet offer-create --rpc http://localhost:38300 ...
```

Or run the wallet binary inside the running container:

```sh
docker-compose exec iriumd /opt/irium/iriumd
```

If you want a full wallet CLI inside the container, use the release archive for
your platform from https://github.com/iriumlabs/irium/releases — the
`irium-wallet` binary runs standalone and connects to any reachable node endpoint.

---

## Pulling images from the registry

Pre-built images are published to GitHub Container Registry on every release:

```sh
# Node
docker pull ghcr.io/iriumlabs/irium:latest

# Miner
docker pull ghcr.io/iriumlabs/irium-miner:latest

# Specific version
docker pull ghcr.io/iriumlabs/irium:v1.1.0
```

---

## Running the node only (no miner)

```sh
docker-compose up -d iriumd
```

---

## Stopping

```sh
# Stop containers, keep data volume
docker-compose down

# Stop containers and delete data volume (re-sync required)
docker-compose down -v
```
