#!/bin/sh
# Usage: curl -sSfL https://raw.githubusercontent.com/Sam-Lane/kip/main/install.sh | sh
# Override install dir:  INSTALL_DIR=~/.local/bin curl -sSfL ... | sh
# Pin a version:         KIP_VERSION=v0.1.0 curl -sSfL ... | sh
set -e

REPO="Sam-Lane/kip"
BINARY="kip"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

# --- detect OS ---
OS=$(uname -s)
case "$OS" in
  Linux)  OS_TRIPLE="unknown-linux-gnu" ;;
  Darwin) OS_TRIPLE="apple-darwin" ;;
  *)
    echo "error: unsupported OS: $OS" >&2
    exit 1
    ;;
esac

# --- detect arch ---
ARCH=$(uname -m)
case "$ARCH" in
  x86_64) ARCH_PREFIX="x86_64" ;;
  *)
    echo "error: unsupported architecture: $ARCH (only x86_64 is currently released)" >&2
    exit 1
    ;;
esac

TARGET="${ARCH_PREFIX}-${OS_TRIPLE}"

# --- resolve version ---
if [ -z "$KIP_VERSION" ]; then
  KIP_VERSION=$(curl -sSfL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' \
    | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
  if [ -z "$KIP_VERSION" ]; then
    echo "error: could not determine latest release version" >&2
    exit 1
  fi
fi

ARCHIVE="${BINARY}-${KIP_VERSION}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${KIP_VERSION}/${ARCHIVE}"

echo "Installing kip ${KIP_VERSION} (${TARGET}) to ${INSTALL_DIR} ..."

# --- download and extract ---
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

curl -sSfL "$URL" | tar xz -C "$TMP"

# --- install ---
if [ -w "$INSTALL_DIR" ]; then
  install -m 755 "$TMP/$BINARY" "$INSTALL_DIR/$BINARY"
else
  echo "note: $INSTALL_DIR is not writable, trying sudo ..."
  sudo install -m 755 "$TMP/$BINARY" "$INSTALL_DIR/$BINARY"
fi

echo "Installed: $(command -v $BINARY) ($KIP_VERSION)"
