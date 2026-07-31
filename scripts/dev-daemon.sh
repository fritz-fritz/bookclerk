#!/usr/bin/env bash
# Build + stage first-party plugins, then run bookclerkd with external guests only.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="${PROFILE:-debug}"
FILES_DIR="${BOOKCLERK_FILES_DIR:-/tmp/BookclerkFiles}"
ARTIFACTS="${BOOKCLERK_PLUGIN_ARTIFACTS:-$ROOT/target/plugin-artifacts}"

"$ROOT/scripts/build-first-party-plugins.sh" "$PROFILE"
"$ROOT/scripts/stage-first-party-plugins.sh" "$PROFILE" "$ARTIFACTS"

export BOOKCLERK_FILES_DIR="$FILES_DIR"
export BOOKCLERK_PLUGIN_DIRS="$ARTIFACTS"

exec cargo run -p bookclerkd -- "$@"
