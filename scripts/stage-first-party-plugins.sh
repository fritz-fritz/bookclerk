#!/usr/bin/env bash
# Stage first-party plugin binaries + plugin.toml for local/CI integration tests.
# Prefer: cargo stage-plugins   (add --release for release profile)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="${1:-debug}"
DEST="${2:-${BOOKCLERK_PLUGIN_ARTIFACTS:-$ROOT/target/plugin-artifacts}}"
args=()
if [[ "$PROFILE" == "release" ]]; then
  args+=(--release)
fi
exec cargo run -p bookclerk-dev --manifest-path "$ROOT/Cargo.toml" -- stage-plugins \
  --dest "$DEST" "${args[@]}"
