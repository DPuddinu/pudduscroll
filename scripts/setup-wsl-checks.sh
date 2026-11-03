#!/usr/bin/env bash
set -euo pipefail

if ! grep -qi microsoft /proc/version 2>/dev/null; then
	if command -v wsl.exe >/dev/null 2>&1; then
		if command -v wslpath >/dev/null 2>&1; then
			repo=$(wslpath -a "$(pwd)")
		else
			repo=$(wsl.exe wslpath -a "$(pwd)" | tr -d "$(printf '\r')")
		fi
		exec wsl.exe -e bash -lc "cd \"$repo\" && exec bash scripts/setup-wsl-checks.sh"
	fi
	echo "Run this inside WSL: wsl -e bash scripts/setup-wsl-checks.sh" >&2
	exit 1
fi

sudo apt-get update
sudo apt-get install -y \
	build-essential \
	curl \
	pkg-config \
	libxkbcommon-dev \
	libwayland-dev \
	libvulkan-dev \
	libxcb-render0-dev \
	libxcb-shape0-dev \
	libxcb-xfixes0-dev

if ! command -v rustup >/dev/null 2>&1; then
	curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
fi

# shellcheck disable=SC1091
source "${HOME}/.cargo/env"
rustup component add rustfmt clippy

echo "WSL checks ready. From Windows, git hooks run these via WSL."
