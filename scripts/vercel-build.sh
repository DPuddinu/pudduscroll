#!/usr/bin/env bash
set -euo pipefail

if [ -f /rust/env ]; then
  # shellcheck source=/dev/null
  source /rust/env
elif [ -f "${HOME}/.cargo/env" ]; then
  # shellcheck source=/dev/null
  source "${HOME}/.cargo/env"
fi

export PATH="${CARGO_HOME:-${HOME}/.cargo}/bin:/rust/bin:${PATH}"
cd crates/app
trunk build --release
