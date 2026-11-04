#!/usr/bin/env bash
# Type-check the workspace for macOS (aarch64-apple-darwin) — including all
# `#[cfg(target_os = "macos")]` code (NSPanel, sleep/wake observers, sound).
# On a Mac this is a plain `cargo check`. On Linux it uses a stub C compiler
# because nothing is linked. This proves the macOS code compiles; it does NOT
# prove it behaves correctly — that still needs manual verification on a Mac.
set -euo pipefail
cd "$(dirname "$0")/.."
TARGET="${1:-aarch64-apple-darwin}"
if [[ "$(uname -s)" == "Darwin" ]]; then
  exec cargo check --workspace --all-targets --target "$TARGET"
fi
rustup target list --installed | grep -q "^${TARGET}$" || rustup target add "$TARGET"
CC_VAR="CC_${TARGET//-/_}"
AR_VAR="AR_${TARGET//-/_}"
export "$CC_VAR=$PWD/scripts/macos-check-cc.sh" "$AR_VAR=ar"
cargo check --workspace --all-targets --target "$TARGET"
cargo clippy --workspace --all-targets --target "$TARGET" -- -D warnings
