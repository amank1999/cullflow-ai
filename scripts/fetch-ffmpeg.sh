#!/usr/bin/env bash
# Downloads a static ffmpeg build for this machine into src-tauri/binaries/,
# named per Tauri's externalBin convention (ffmpeg-<target-triple>). Run this
# once before `npm run tauri dev` / `npm run tauri build` on macOS or Linux.
# Windows: use scripts/fetch-ffmpeg.ps1 instead.
set -euo pipefail

DEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/src-tauri/binaries"
mkdir -p "$DEST_DIR"

OS="$(uname -s)"
ARCH="$(uname -m)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

case "$OS" in
  Darwin)
    case "$ARCH" in
      arm64) TRIPLE="aarch64-apple-darwin" ;;
      x86_64) TRIPLE="x86_64-apple-darwin" ;;
      *) echo "Unsupported macOS architecture: $ARCH" >&2; exit 1 ;;
    esac
    echo "Downloading ffmpeg for macOS ($TRIPLE)..."
    curl -fL "https://evermeet.cx/ffmpeg/getrelease/zip" -o "$TMP/ffmpeg.zip"
    unzip -oq "$TMP/ffmpeg.zip" -d "$TMP"
    cp "$TMP/ffmpeg" "$DEST_DIR/ffmpeg-$TRIPLE"
    chmod +x "$DEST_DIR/ffmpeg-$TRIPLE"
    ;;
  Linux)
    case "$ARCH" in
      x86_64) TRIPLE="x86_64-unknown-linux-gnu"; PKG_ARCH="amd64" ;;
      aarch64) TRIPLE="aarch64-unknown-linux-gnu"; PKG_ARCH="arm64" ;;
      *) echo "Unsupported Linux architecture: $ARCH" >&2; exit 1 ;;
    esac
    echo "Downloading ffmpeg for Linux ($TRIPLE)..."
    curl -fL "https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-${PKG_ARCH}-static.tar.xz" -o "$TMP/ffmpeg.tar.xz"
    tar -xf "$TMP/ffmpeg.tar.xz" -C "$TMP"
    cp "$TMP"/ffmpeg-*-static/ffmpeg "$DEST_DIR/ffmpeg-$TRIPLE"
    chmod +x "$DEST_DIR/ffmpeg-$TRIPLE"
    ;;
  *)
    echo "This script supports macOS and Linux. On Windows, run scripts/fetch-ffmpeg.ps1 instead." >&2
    exit 1
    ;;
esac

echo "Bundled ffmpeg for $TRIPLE -> $DEST_DIR/ffmpeg-$TRIPLE"
