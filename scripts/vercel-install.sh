#!/usr/bin/env bash
set -euo pipefail

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
# shellcheck source=/dev/null
source "${HOME}/.cargo/env"
rustup target add wasm32-unknown-unknown
cargo install trunk --locked --version 0.22.1
