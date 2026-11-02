#!/usr/bin/env bash
set -euo pipefail

if [ -f /rust/env ]; then
  # shellcheck source=/dev/null
  source /rust/env
elif [ -f "${HOME}/.cargo/env" ]; then
  # shellcheck source=/dev/null
  source "${HOME}/.cargo/env"
elif ! command -v cargo >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
  # shellcheck source=/dev/null
  source "${HOME}/.cargo/env"
fi

export PATH="${CARGO_HOME:-${HOME}/.cargo}/bin:/rust/bin:${PATH}"
rustup target add wasm32-unknown-unknown
if ! command -v trunk >/dev/null 2>&1; then
  cargo install trunk --locked --version 0.22.1
fi
