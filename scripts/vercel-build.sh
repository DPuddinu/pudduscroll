#!/usr/bin/env bash
set -euo pipefail

# shellcheck source=/dev/null
source "${HOME}/.cargo/env"
cd crates/app
trunk build --release
