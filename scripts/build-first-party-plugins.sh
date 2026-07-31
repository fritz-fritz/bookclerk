#!/usr/bin/env bash
# Build all first-party external plugin binaries (debug or release).
# Prefer: cargo build-plugins   (add --release for release profile)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="${1:-debug}"
args=()
if [[ "$PROFILE" == "release" ]]; then
  args+=(--release)
fi
exec cargo run -p bookclerk-dev --manifest-path "$ROOT/Cargo.toml" -- build-plugins "${args[@]}"
