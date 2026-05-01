#!/bin/bash
set -euo pipefail

GITHUB_REPO="iriumlabs/irium"
INSTALL_DIR="/usr/local/bin"
CORE_BINS="iriumd irium-wallet irium-miner irium-spv"

# ── helpers ──────────────────────────────────────────────────────────────────

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

detect_platform() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "${os}" in
    Linux)
      case "${arch}" in
        x86_64)  echo "x86_64-unknown-linux-gnu" ;;
        aarch64) echo "aarch64-unknown-linux-gnu" ;;
        *)       echo "" ;;
      esac ;;
    Darwin)
      case "${arch}" in
        x86_64) echo "x86_64-apple-darwin" ;;
        arm64)  echo "aarch64-apple-darwin" ;;
        *)      echo "" ;;
      esac ;;
    *)
      echo "" ;;
  esac
}

# ── init ─────────────────────────────────────────────────────────────────────

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

SERVICE_USER="${IRIUM_SERVICE_USER:-}"
if [ -z "${SERVICE_USER}" ]; then
  SERVICE_USER="${SUDO_USER:-$(whoami)}"
fi

if [ "$(id -u)" = "0" ]; then
  SUDO=""
else
  SUDO="sudo"
fi

echo ""
echo "==> Irium Installer"
echo ""

PLATFORM="$(detect_platform)"
BUILT_FROM_SOURCE="false"

# ── try pre-built binary ──────────────────────────────────────────────────────

if [ -n "${PLATFORM}" ]; then
  echo "==> Platform: ${PLATFORM}"
  echo "==> Fetching latest release..."

  LATEST_TAG="$(curl -fsSL "https://api.github.com/repos/${GITHUB_REPO}/releases/latest" \
    | grep '"tag_name"' | head -1 \
    | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')"

  if [ -n "${LATEST_TAG}" ]; then
    ARCHIVE_NAME="irium-${LATEST_TAG}-${PLATFORM}.tar.gz"
    BASE_URL="https://github.com/${GITHUB_REPO}/releases/download/${LATEST_TAG}"
    DOWNLOAD_URL="${BASE_URL}/${ARCHIVE_NAME}"
    CHECKSUM_URL="${BASE_URL}/checksums.txt"

    echo "==> Release:  ${LATEST_TAG}"
    echo "==> Archive:  ${ARCHIVE_NAME}"

    TMPDIR_IRM="$(mktemp -d)"
    trap 'rm -rf "${TMPDIR_IRM}"' EXIT

    echo "==> Downloading ${ARCHIVE_NAME}..."
    if curl -fL --progress-bar -o "${TMPDIR_IRM}/${ARCHIVE_NAME}" "${DOWNLOAD_URL}"; then
      echo "==> Downloading checksums.txt..."
      if curl -fsSL -o "${TMPDIR_IRM}/checksums.txt" "${CHECKSUM_URL}"; then
        EXPECTED_HASH="$(grep "${ARCHIVE_NAME}" "${TMPDIR_IRM}/checksums.txt" | awk '{print $1}')"
        if [ -n "${EXPECTED_HASH}" ]; then
          echo "==> Verifying SHA256..."
          ACTUAL_HASH="$(sha256_file "${TMPDIR_IRM}/${ARCHIVE_NAME}")"
          if [ "${ACTUAL_HASH}" = "${EXPECTED_HASH}" ]; then
            echo "==> Checksum OK: ${ACTUAL_HASH}"
            echo "==> Extracting..."
            mkdir -p "${TMPDIR_IRM}/bin"
            tar -xzf "${TMPDIR_IRM}/${ARCHIVE_NAME}" -C "${TMPDIR_IRM}/bin"
            echo "==> Installing to ${INSTALL_DIR}..."
            for bin in ${CORE_BINS}; do
              if [ -f "${TMPDIR_IRM}/bin/${bin}" ]; then
                ${SUDO} install -m 0755 "${TMPDIR_IRM}/bin/${bin}" "${INSTALL_DIR}/${bin}"
                echo "    + ${bin}"
              fi
            done
            if [ -f "${TMPDIR_IRM}/bin/irium-miner-gpu" ]; then
              ${SUDO} install -m 0755 "${TMPDIR_IRM}/bin/irium-miner-gpu" "${INSTALL_DIR}/irium-miner-gpu"
              echo "    + irium-miner-gpu"
            fi
            echo ""
            echo "==> Installed from pre-built release ${LATEST_TAG}."
          else
            echo "==> WARNING: Checksum mismatch."
            echo "    Expected: ${EXPECTED_HASH}"
            echo "    Got:      ${ACTUAL_HASH}"
            echo "==> Falling back to source build."
            BUILT_FROM_SOURCE="true"
          fi
        else
          echo "==> WARNING: No checksum entry for ${ARCHIVE_NAME}. Falling back to source build."
          BUILT_FROM_SOURCE="true"
        fi
      else
        echo "==> WARNING: Could not download checksums.txt. Falling back to source build."
        BUILT_FROM_SOURCE="true"
      fi
    else
      echo "==> No pre-built binary for ${PLATFORM} in this release. Falling back to source build."
      BUILT_FROM_SOURCE="true"
    fi
  else
    echo "==> WARNING: Could not determine latest release. Falling back to source build."
    BUILT_FROM_SOURCE="true"
  fi
else
  echo "==> Platform not supported for pre-built binaries. Building from source."
  BUILT_FROM_SOURCE="true"
fi

# ── source build fallback ─────────────────────────────────────────────────────

if [ "${BUILT_FROM_SOURCE}" = "true" ]; then
  echo ""
  echo "==> Building from source..."
  if ! command -v cargo >/dev/null 2>&1; then
    echo "ERROR: Rust toolchain not found. Install from https://rustup.rs"
    exit 1
  fi
  cd "${ROOT_DIR}"
  echo "==> cargo build --release (this may take several minutes)..."
  cargo build --release
  echo "==> Installing to ${INSTALL_DIR}..."
  for bin in ${CORE_BINS}; do
    if [ -f "target/release/${bin}" ]; then
      ${SUDO} install -m 0755 "target/release/${bin}" "${INSTALL_DIR}/${bin}"
      echo "    + ${bin}"
    fi
  done
  if [ -f "target/release/irium-miner-gpu" ]; then
    ${SUDO} install -m 0755 "target/release/irium-miner-gpu" "${INSTALL_DIR}/irium-miner-gpu"
    echo "    + irium-miner-gpu"
  fi
  echo "==> Binaries installed from source."
fi

# ── systemd setup ─────────────────────────────────────────────────────────────

if ! command -v systemctl >/dev/null 2>&1; then
  echo ""
  echo "==> systemctl not found; skipping systemd setup."
  echo "==> Start iriumd manually: ${INSTALL_DIR}/iriumd"
  exit 0
fi

if [ ! -f "${ROOT_DIR}/systemd/iriumd.service" ]; then
  echo ""
  echo "==> Systemd unit files not found in ${ROOT_DIR}/systemd/."
  echo "==> Skipping service setup. Run install.sh from the repository directory to configure services."
  exit 0
fi

echo ""
echo "==> Installing systemd services (user: ${SERVICE_USER})..."

${SUDO} mkdir -p /etc/irium
for unit in iriumd irium-miner irium-explorer irium-wallet-api; do
  ${SUDO} cp "${ROOT_DIR}/systemd/${unit}.service" "/etc/systemd/system/${unit}.service"
done
${SUDO} sed -i "s|@IRIUM_HOME@|${ROOT_DIR}|g" \
  /etc/systemd/system/iriumd.service \
  /etc/systemd/system/irium-miner.service \
  /etc/systemd/system/irium-explorer.service \
  /etc/systemd/system/irium-wallet-api.service
${SUDO} sed -i "s|@IRIUM_USER@|${SERVICE_USER}|g" \
  /etc/systemd/system/iriumd.service \
  /etc/systemd/system/irium-miner.service \
  /etc/systemd/system/irium-explorer.service \
  /etc/systemd/system/irium-wallet-api.service

[ -f /etc/irium/iriumd.env ]     || ${SUDO} cp "${ROOT_DIR}/systemd/iriumd.env.example"     /etc/irium/iriumd.env
[ -f /etc/irium/miner.env ]      || ${SUDO} cp "${ROOT_DIR}/systemd/miner.env.example"       /etc/irium/miner.env
[ -f /etc/irium/explorer.env ]   || ${SUDO} cp "${ROOT_DIR}/systemd/explorer.env.example"    /etc/irium/explorer.env
[ -f /etc/irium/wallet-api.env ] || ${SUDO} cp "${ROOT_DIR}/systemd/wallet-api.env.example"  /etc/irium/wallet-api.env

${SUDO} sed -i "s|^IRIUM_HOME=.*|IRIUM_HOME=${ROOT_DIR}|" /etc/irium/iriumd.env
${SUDO} sed -i "s|^IRIUM_NODE_CONFIG=.*|IRIUM_NODE_CONFIG=${ROOT_DIR}/configs/node.json|" /etc/irium/iriumd.env
${SUDO} sed -i "s|^IRIUM_HOME=.*|IRIUM_HOME=${ROOT_DIR}|" /etc/irium/miner.env

${SUDO} systemctl daemon-reload
${SUDO} systemctl enable --now iriumd.service

echo ""
echo "==> Node service installed and started."
echo "    Edit /etc/irium/miner.env and set IRIUM_MINER_ADDRESS, then:"
echo "    sudo systemctl enable --now irium-miner.service"
echo "    Optional: sudo systemctl enable --now irium-explorer.service"
echo "    Optional: sudo systemctl enable --now irium-wallet-api.service"
