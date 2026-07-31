#!/usr/bin/env bash
# Stage first-party plugin binaries + plugin.toml for local/CI integration tests.
# Does NOT publish artifacts — only copies build outputs into a plugins tree.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="${1:-debug}"
DEST="${2:-${BOOKCLERK_PLUGIN_ARTIFACTS:-$ROOT/target/plugin-artifacts}}"
BIN_DIR="$ROOT/target/$PROFILE"

# bin:kind:id:srcdir[:manifest.toml basename under srcdir]
PLUGINS=(
  "bookclerk-plugin-echo-integration:integration:echo:crates/bookclerk-plugin-examples/echo-integration"
  "bookclerk-plugin-source-audible:source:audible:crates/bookclerk-plugins/source-audible"
  "bookclerk-plugin-source-libro:source:libro:crates/bookclerk-plugins/source-libro"
  "bookclerk-plugin-source-chirp:source:chirp:crates/bookclerk-plugins/source-chirp"
  "bookclerk-plugin-source-graphicaudio:source:graphicaudio:crates/bookclerk-plugins/source-graphicaudio"
  "bookclerk-plugin-integration-audiobookshelf:integration:audiobookshelf:crates/bookclerk-plugins/integration-audiobookshelf"
  "bookclerk-plugin-destination-s3:output:s3:crates/bookclerk-plugins/destination-s3"
  "bookclerk-plugin-destination-local:output:local:crates/bookclerk-plugins/destination-local"
  "bookclerk-plugin-database:database:sqlite:crates/bookclerk-plugins/database:plugin.toml"
  "bookclerk-plugin-database:database:d1:crates/bookclerk-plugins/database:plugin-d1.toml"
  "bookclerk-plugin-database:database:postgres:crates/bookclerk-plugins/database:plugin-postgres.toml"
)

mkdir -p "$DEST"

patch_command() {
  local manifest="$1"
  local bin_name="$2"
  if command -v python3 >/dev/null 2>&1; then
    python3 - "$manifest" "$bin_name" <<'PY'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
bin_name = sys.argv[2]
text = path.read_text()
lines = []
for line in text.splitlines():
    if line.strip().startswith("command"):
        lines.append(f'command = "./{bin_name}"')
    else:
        lines.append(line)
path.write_text("\n".join(lines) + "\n")
PY
  fi
}

for entry in "${PLUGINS[@]}"; do
  IFS=':' read -r bin _kind id srcdir manifest <<<"$entry"
  manifest="${manifest:-plugin.toml}"
  src_bin="$BIN_DIR/$bin"
  if [[ ! -x "$src_bin" && ! -f "$src_bin" ]]; then
    if [[ -f "${src_bin}.exe" ]]; then
      src_bin="${src_bin}.exe"
    else
      echo "missing binary: $BIN_DIR/$bin (run ./scripts/build-first-party-plugins.sh $PROFILE)" >&2
      exit 1
    fi
  fi
  out="$DEST/$id"
  mkdir -p "$out"
  cp -f "$src_bin" "$out/"
  chmod +x "$out/$(basename "$src_bin")" 2>/dev/null || true
  manifest_src="$ROOT/$srcdir/$manifest"
  if [[ ! -f "$manifest_src" ]]; then
    echo "missing manifest for $id: $manifest_src" >&2
    exit 1
  fi
  cp -f "$manifest_src" "$out/plugin.toml"
  patch_command "$out/plugin.toml" "$(basename "$src_bin")"
  echo "staged $id -> $out"
done

echo "BOOKCLERK_PLUGIN_ARTIFACTS=$DEST"
