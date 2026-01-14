#!/bin/bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"

SERVICE_USER="${IRIUM_SERVICE_USER:-}"
if [ -z "$SERVICE_USER" ]; then
  if [ -n "${SUDO_USER:-}" ]; then
    SERVICE_USER="$SUDO_USER"
  else
    SERVICE_USER="$(whoami)"
  fi
fi

echo "🚀 Installing Irium (Rust)"

if ! command -v cargo >/dev/null 2>&1; then
  echo "❌ Rust toolchain not found. Install Rust from https://rustup.rs"
  exit 1
fi

cd "$ROOT_DIR"

echo "📦 Building release binaries..."
cargo build --release

if ! command -v systemctl >/dev/null 2>&1; then
  echo "⚠️ systemctl not found; skipping systemd setup."
  exit 0
fi

echo "🧩 Installing systemd units..."
echo "Using systemd service user: $SERVICE_USER (override with IRIUM_SERVICE_USER)"
sudo mkdir -p /etc/irium
sudo cp systemd/iriumd.service /etc/systemd/system/iriumd.service
sudo cp systemd/irium-miner.service /etc/systemd/system/irium-miner.service
sudo cp systemd/irium-explorer.service /etc/systemd/system/irium-explorer.service
sudo cp systemd/irium-wallet-api.service /etc/systemd/system/irium-wallet-api.service
sudo sed -i "s|@IRIUM_HOME@|$ROOT_DIR|g" /etc/systemd/system/iriumd.service /etc/systemd/system/irium-miner.service /etc/systemd/system/irium-explorer.service /etc/systemd/system/irium-wallet-api.service
sudo sed -i "s|@IRIUM_USER@|$SERVICE_USER|g" /etc/systemd/system/iriumd.service /etc/systemd/system/irium-miner.service /etc/systemd/system/irium-explorer.service /etc/systemd/system/irium-wallet-api.service

if [ ! -f /etc/irium/iriumd.env ]; then
  sudo cp systemd/iriumd.env.example /etc/irium/iriumd.env
fi
if [ ! -f /etc/irium/miner.env ]; then
  sudo cp systemd/miner.env.example /etc/irium/miner.env
fi
if [ ! -f /etc/irium/explorer.env ]; then
  sudo cp systemd/explorer.env.example /etc/irium/explorer.env
fi
if [ ! -f /etc/irium/wallet-api.env ]; then
  sudo cp systemd/wallet-api.env.example /etc/irium/wallet-api.env
fi

sudo sed -i "s|^IRIUM_HOME=.*|IRIUM_HOME=$ROOT_DIR|" /etc/irium/iriumd.env
sudo sed -i "s|^IRIUM_NODE_CONFIG=.*|IRIUM_NODE_CONFIG=$ROOT_DIR/configs/node.json|" /etc/irium/iriumd.env
sudo sed -i "s|^IRIUM_HOME=.*|IRIUM_HOME=$ROOT_DIR|" /etc/irium/miner.env
sudo sed -i "s|^IRIUM_NODE_RPC=.*|IRIUM_NODE_RPC=http://127.0.0.1:38300|" /etc/irium/explorer.env /etc/irium/wallet-api.env

sudo systemctl daemon-reload
sudo systemctl enable --now iriumd.service

echo "✅ Node service installed and started."
echo "➡️ Edit /etc/irium/miner.env to set IRIUM_MINER_ADDRESS."
echo "➡️ Then enable the miner: sudo systemctl enable --now irium-miner.service"
echo "➡️ Optional: configure /etc/irium/explorer.env and enable irium-explorer.service"
echo "➡️ Optional: configure /etc/irium/wallet-api.env and enable irium-wallet-api.service"
