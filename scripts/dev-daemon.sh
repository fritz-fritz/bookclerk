#!/usr/bin/env bash
# Build + stage first-party plugins, then run bookclerkd with external guests only.
# Prefer: cargo dev-daemon
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
exec cargo run -p bookclerk-dev --manifest-path "$ROOT/Cargo.toml" -- dev-daemon "$@"
