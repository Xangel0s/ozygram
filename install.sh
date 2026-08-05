#!/usr/bin/env bash
# Ozygram / Ozymem Automatic Release Installer for Linux & macOS
set -e

echo "======================================================"
echo "   Ozymem / Ozygram Release Installer (Linux/macOS)  "
echo "======================================================"

OZY_DIR="$HOME/.ozymem"
BIN_DIR="$OZY_DIR/bin"
PY_DIR="$OZY_DIR/python/ozy-brain"

echo "[1/4] Preparing installation directories..."
mkdir -p "$BIN_DIR"
mkdir -p "$OZY_DIR/python"

echo "[2/4] Building release binaries (cargo build --release)..."
cargo build --release

SERVER_RELEASE="./target/release/ozymem-server"
CLI_RELEASE="./target/release/ozymem"

if [ ! -f "$SERVER_RELEASE" ] || [ ! -f "$CLI_RELEASE" ]; then
    echo "[!] Release binaries not found in ./target/release."
    exit 1
fi

echo "[3/4] Installing executables and Ozy Brain worker..."
cp -f "$SERVER_RELEASE" "$BIN_DIR/ozymem-server"
cp -f "$CLI_RELEASE" "$BIN_DIR/ozymem"
chmod +x "$BIN_DIR/ozymem-server" "$BIN_DIR/ozymem"

if [ -d "./python/ozy-brain" ]; then
    cp -rf "./python/ozy-brain" "$OZY_DIR/python/"
fi

echo "[4/4] Release installation completed successfully!"
echo ""
echo "Add to your shell profile (~/.bashrc or ~/.zshrc):"
echo "  export PATH=\"$BIN_DIR:\$PATH\""
echo "  export OZY_BRAIN_PATH=\"$PY_DIR\""
echo ""
echo "MCP Server Configuration (mcp_servers):"
echo "{"
echo "  \"ozygram\": {"
echo "    \"command\": \"$BIN_DIR/ozymem-server\","
echo "    \"args\": []"
echo "  }"
echo "}"
