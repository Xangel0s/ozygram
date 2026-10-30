#!/usr/bin/env bash
# Ozygram / Ozymem Automatic Release Installer for Linux & macOS
# Run via: ./install.sh
# Or one-liner: curl -fsSL https://raw.githubusercontent.com/Xangel0s/ozygram/main/install.sh | bash
set -e

FROM_SOURCE=false
for arg in "$@"; do
    if [ "$arg" = "--from-source" ]; then
        FROM_SOURCE=true
    fi
done

echo "======================================================"
echo "   Ozymem / Ozygram Release Installer (Linux/macOS)  "
echo "======================================================"

OZY_DIR="$HOME/.ozymem"
BIN_DIR="$OZY_DIR/bin"
PY_DIR="$OZY_DIR/python/ozy-brain"

echo "[1/4] Preparing installation directories..."
mkdir -p "$BIN_DIR"
mkdir -p "$OZY_DIR/python"

INSTALLED_FROM_RELEASE=false

if [ "$FROM_SOURCE" = false ] && [ ! -f "./target/release/ozymem" ]; then
    echo "[2/4] Downloading precompiled release from GitHub..."
    OS="$(uname -s)"
    ARCH="$(uname -m)"
    ASSET_NAME=""

    if [ "$OS" = "Linux" ] && [ "$ARCH" = "x86_64" ]; then
        ASSET_NAME="ozymem-linux-x64.tar.gz"
    elif [ "$OS" = "Darwin" ] && [ "$ARCH" = "arm64" ]; then
        ASSET_NAME="ozymem-macos-arm64.tar.gz"
    elif [ "$OS" = "Darwin" ]; then
        ASSET_NAME="ozymem-macos-x64.tar.gz"
    fi

    if [ -n "$ASSET_NAME" ]; then
        TMP_DIR="$(mktemp -d)"
        DOWNLOAD_URL="https://github.com/Xangel0s/ozygram/releases/latest/download/$ASSET_NAME"
        SHA_URL="https://github.com/Xangel0s/ozygram/releases/latest/download/SHA256SUMS"
        echo " -> Fetching $DOWNLOAD_URL..."
        if curl -fsSL "$DOWNLOAD_URL" -o "$TMP_DIR/$ASSET_NAME"; then
            if curl -fsSL "$SHA_URL" -o "$TMP_DIR/SHA256SUMS" 2>/dev/null; then
                cd "$TMP_DIR"
                if grep "$ASSET_NAME" SHA256SUMS >/dev/null 2>&1; then
                    if command -v sha256sum >/dev/null 2>&1; then
                        grep "$ASSET_NAME" SHA256SUMS | sha256sum -c - && echo " [STATUS: VERIFIED] SHA256 checksum verified."
                    elif command -v shasum >/dev/null 2>&1; then
                        grep "$ASSET_NAME" SHA256SUMS | shasum -a 256 -c - && echo " [STATUS: VERIFIED] SHA256 checksum verified."
                    fi
                fi
                cd - >/dev/null
            fi
            tar -xzf "$TMP_DIR/$ASSET_NAME" -C "$BIN_DIR/"
            chmod +x "$BIN_DIR/ozymem-server" "$BIN_DIR/ozymem" "$BIN_DIR/ozygram" 2>/dev/null || true
            if [ -f "$BIN_DIR/ozy-brain" ]; then
                chmod +x "$BIN_DIR/ozy-brain"
            fi
            rm -rf "$TMP_DIR"
            echo " -> Successfully installed precompiled binaries into $BIN_DIR"
            INSTALLED_FROM_RELEASE=true
        else
            echo " [WARN] Download failed, falling back to source build..."
        fi
    fi
fi

if [ "$INSTALLED_FROM_RELEASE" = false ]; then
    echo "[2/4] Building release binaries (cargo build --release)..."
    cargo build --release

    PRIMARY_RELEASE="./target/release/ozygram"
    if [ ! -f "$PRIMARY_RELEASE" ]; then
        PRIMARY_RELEASE="./target/release/ozymem"
    fi
    SERVER_RELEASE="./target/release/ozymem-server"

    if [ ! -f "$PRIMARY_RELEASE" ]; then
        echo "[!] Release binary not found in ./target/release."
        exit 1
    fi

    echo "[3/4] Installing executables and Ozy Brain worker..."
    cp -f "$PRIMARY_RELEASE" "$BIN_DIR/ozygram"
    cp -f "$PRIMARY_RELEASE" "$BIN_DIR/ozymem"
    if [ -f "$SERVER_RELEASE" ]; then
        cp -f "$SERVER_RELEASE" "$BIN_DIR/ozymem-server"
    else
        cp -f "$PRIMARY_RELEASE" "$BIN_DIR/ozymem-server"
    fi
    chmod +x "$BIN_DIR/ozymem-server" "$BIN_DIR/ozymem" "$BIN_DIR/ozygram"

    if [ -f "./target/release/ozy-brain" ]; then
        cp -f "./target/release/ozy-brain" "$BIN_DIR/ozy-brain"
        chmod +x "$BIN_DIR/ozy-brain"
        echo " -> Deployed standalone ozy-brain (Zero-Python runtime bundled)."
    fi
fi

if [ -d "./python/ozy-brain" ]; then
    cp -rf "./python/ozy-brain" "$OZY_DIR/python/"
fi

echo "[4/4] Release installation completed successfully!"
echo ""
echo "Add to your shell profile (~/.bashrc or ~/.zshrc):"
echo "  export PATH=\"$BIN_DIR:\$PATH\""
echo "  export OZY_BRAIN_PATH=\"$PY_DIR\""
echo ""
echo "[Auto-Configuration] Scanning and auto-configuring installed AI clients..."
"$BIN_DIR/ozymem" init --auto || true
