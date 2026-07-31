#!/usr/bin/env bash
# Build all first-party external plugin binaries (debug or release).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="${1:-debug}"

cd "$ROOT"

PKGS=(
  bookclerk-plugin-echo-integration
  bookclerk-plugin-source-audible
  bookclerk-plugin-source-libro
  bookclerk-plugin-source-chirp
  bookclerk-plugin-source-graphicaudio
  bookclerk-plugin-integration-audiobookshelf
  bookclerk-plugin-destination-s3
  bookclerk-plugin-database
)

if [[ "$PROFILE" == "release" ]]; then
  args=(--release)
else
  args=()
fi

build_args=()
for pkg in "${PKGS[@]}"; do
  build_args+=(-p "$pkg")
done
cargo build "${args[@]}" "${build_args[@]}"

echo "built first-party plugins ($PROFILE) under $ROOT/target/$PROFILE/"
