#!/bin/bash
# Run this inside WSL2 Ubuntu to set up the toolchain.
# Usage: bash setup.sh

set -e

echo "=== Installing Rust (if not present) ==="
if ! command -v rustup &> /dev/null; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

echo "=== Setting nightly toolchain ==="
rustup override set nightly
rustup component add rust-src llvm-tools

echo "=== Installing bootimage ==="
cargo install bootimage

echo "=== Installing QEMU ==="
sudo apt update && sudo apt install -y qemu-system-x86

echo "=== Verifying ==="
rustup show
cargo bootimage --version
qemu-system-x86_64 --version

echo ""
echo "Setup complete! Run: cargo bootimage run"
