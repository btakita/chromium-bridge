#!/bin/sh
# Install chromium-bridge from GitHub Releases
# Usage: curl -sSf https://raw.githubusercontent.com/btakita/chromium-bridge/main/install.sh | sh
set -e

REPO="btakita/chromium-bridge"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux)  TARGET_OS="unknown-linux-gnu" ;;
    Darwin) TARGET_OS="apple-darwin" ;;
    *)      echo "Unsupported OS: $OS" >&2; exit 1 ;;
esac

case "$ARCH" in
    x86_64|amd64)  TARGET_ARCH="x86_64" ;;
    aarch64|arm64) TARGET_ARCH="aarch64" ;;
    *)             echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

TARGET="${TARGET_ARCH}-${TARGET_OS}"

# Get latest version
VERSION="$(curl -sSf "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed 's/.*"tag_name": *"\(.*\)".*/\1/')"
if [ -z "$VERSION" ]; then
    echo "Failed to detect latest version" >&2
    exit 1
fi

FILENAME="chromium-bridge-${VERSION}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${VERSION}/${FILENAME}"

echo "Installing chromium-bridge ${VERSION} for ${TARGET}..."

# Download and extract
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

if ! curl -sSfL "$URL" -o "$TMPDIR/$FILENAME"; then
    echo "No prebuilt binary for ${TARGET}. Install via cargo instead:" >&2
    echo "  cargo install chromium-bridge" >&2
    exit 1
fi

tar xzf "$TMPDIR/$FILENAME" -C "$TMPDIR"

# Install
if [ -w "$INSTALL_DIR" ]; then
    mv "$TMPDIR/chromium-bridge" "$INSTALL_DIR/chromium-bridge"
else
    echo "Installing to $INSTALL_DIR (requires sudo)..."
    sudo mv "$TMPDIR/chromium-bridge" "$INSTALL_DIR/chromium-bridge"
fi

echo "Installed chromium-bridge ${VERSION} to ${INSTALL_DIR}/chromium-bridge"
chromium-bridge --version 2>/dev/null || true
