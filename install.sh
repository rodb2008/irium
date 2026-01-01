#!/bin/bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"

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
sudo mkdir -p /etc/irium
sudo cp systemd/iriumd.service /etc/systemd/system/iriumd.service
sudo cp systemd/irium-miner.service /etc/systemd/system/irium-miner.service
sudo sed -i "s|@IRIUM_HOME@|$ROOT_DIR|g" /etc/systemd/system/iriumd.service /etc/systemd/system/irium-miner.service

if [ ! -f /etc/irium/iriumd.env ]; then
  sudo cp systemd/iriumd.env.example /etc/irium/iriumd.env
fi
if [ ! -f /etc/irium/miner.env ]; then
  sudo cp systemd/miner.env.example /etc/irium/miner.env
fi

sudo sed -i "s|^IRIUM_HOME=.*|IRIUM_HOME=$ROOT_DIR|" /etc/irium/iriumd.env
sudo sed -i "s|^IRIUM_NODE_CONFIG=.*|IRIUM_NODE_CONFIG=$ROOT_DIR/configs/node.json|" /etc/irium/iriumd.env
sudo sed -i "s|^IRIUM_HOME=.*|IRIUM_HOME=$ROOT_DIR|" /etc/irium/miner.env

sudo systemctl daemon-reload
sudo systemctl enable --now iriumd.service

echo "✅ Node service installed and started."
echo "➡️ Edit /etc/irium/miner.env to set IRIUM_MINER_ADDRESS."
echo "➡️ Then enable the miner: sudo systemctl enable --now irium-miner.service"
