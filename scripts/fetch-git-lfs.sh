#!/usr/bin/env bash
#
# Fetch the git-lfs binary for a Rust target triple and place it at
# `src-tauri/bin/git-lfs-<triple>` (or `.exe` on Windows). This is the
# layout Tauri's `bundle.externalBin` expects for sidecars.
#
# Used by CI before `tauri build` and available locally for anyone
# preparing a release build.
#
# Usage:
#   scripts/fetch-git-lfs.sh <target-triple>
#
# Supported triples:
#   x86_64-unknown-linux-gnu
#   x86_64-pc-windows-msvc
#   aarch64-apple-darwin
#   x86_64-apple-darwin
#   universal-apple-darwin   (fetches both arches and lipo-merges them;
#                             requires macOS for the `lipo` tool)

set -euo pipefail

GIT_LFS_VERSION="${GIT_LFS_VERSION:-v3.7.1}"
TARGET="${1:-}"

if [[ -z "$TARGET" ]]; then
  echo "error: target triple required (e.g. x86_64-unknown-linux-gnu)" >&2
  exit 2
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="$REPO_ROOT/src-tauri/bin"

# Universal darwin is fetched as both per-arch binaries, merged via lipo.
if [[ "$TARGET" == "universal-apple-darwin" ]]; then
  if ! command -v lipo >/dev/null 2>&1; then
    echo "error: 'lipo' not found — universal-apple-darwin can only be built on macOS" >&2
    exit 2
  fi
  DEST="$BIN_DIR/git-lfs-${TARGET}"
  if [[ -f "$DEST" ]]; then
    echo "git-lfs sidecar already present at $DEST"
    exit 0
  fi
  "$0" aarch64-apple-darwin
  "$0" x86_64-apple-darwin
  mkdir -p "$BIN_DIR"
  lipo -create \
    "$BIN_DIR/git-lfs-aarch64-apple-darwin" \
    "$BIN_DIR/git-lfs-x86_64-apple-darwin" \
    -output "$DEST"
  chmod +x "$DEST"
  echo "installed universal sidecar at $DEST"
  exit 0
fi

case "$TARGET" in
  x86_64-unknown-linux-gnu)   PLATFORM="linux"   ; ARCH="amd64" ; EXT="tar.gz" ; BIN_SUFFIX="" ;;
  x86_64-pc-windows-msvc)     PLATFORM="windows" ; ARCH="amd64" ; EXT="zip"    ; BIN_SUFFIX=".exe" ;;
  aarch64-apple-darwin)       PLATFORM="darwin"  ; ARCH="arm64" ; EXT="zip"    ; BIN_SUFFIX="" ;;
  x86_64-apple-darwin)        PLATFORM="darwin"  ; ARCH="amd64" ; EXT="zip"    ; BIN_SUFFIX="" ;;
  *)
    echo "error: unsupported target triple '$TARGET'" >&2
    exit 2
    ;;
esac

DEST="$BIN_DIR/git-lfs-${TARGET}${BIN_SUFFIX}"

if [[ -f "$DEST" ]]; then
  echo "git-lfs sidecar already present at $DEST"
  exit 0
fi

mkdir -p "$BIN_DIR"

ARCHIVE_NAME="git-lfs-${PLATFORM}-${ARCH}-${GIT_LFS_VERSION}.${EXT}"
URL="https://github.com/git-lfs/git-lfs/releases/download/${GIT_LFS_VERSION}/${ARCHIVE_NAME}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "downloading $URL"
curl -fsSL "$URL" -o "$WORK/$ARCHIVE_NAME"

cd "$WORK"
case "$EXT" in
  tar.gz) tar -xzf "$ARCHIVE_NAME" ;;
  zip)    unzip -q "$ARCHIVE_NAME" ;;
esac

# Archive layout: `git-lfs-<version>/git-lfs[.exe]` plus docs etc.
SRC_BIN="$(find . -type f -name "git-lfs${BIN_SUFFIX}" | head -n 1)"
if [[ -z "$SRC_BIN" ]]; then
  echo "error: git-lfs binary not found inside archive" >&2
  exit 1
fi

cp "$SRC_BIN" "$DEST"
chmod +x "$DEST"
echo "installed sidecar at $DEST"
