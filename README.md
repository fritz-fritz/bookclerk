# Libation (Rust)

Headless-first Audible library manager — a greenfield Rust rewrite of
[Libation](https://github.com/rmcrackan/Libation), built on
[audible-rs](https://github.com/mkb79/audible-rs).

## Status

**Phase 1 (headless) is feature-complete** for the core loop: auth, library
scan, liberate (Adrm aaxc → m4b), local/S3 storage, CLI, and `libationd`
(scheduler + HTTP control plane).

### Phase 1 checklist

| Capability | Status |
| --- | --- |
| Auth login (QR / callback server / external paste) | done |
| Auth list / status / import (`.auth` + AccountsSettings.json) | done |
| Migrate classic Libation Files (Settings / accounts / DB) | done |
| Library scan → SQLite | done |
| Liberate: license → download → aaxclean decrypt → store | done |
| `library get-license` / `list` / `set-status` | done |
| Local FS + S3/MinIO storage (`put_file` streaming) | done |
| `libationd` scheduled scan + auto-liberate | done |
| `libationd` HTTP `/scan` `/liberate` `/jobs` `/status` | done |
| Widevine/CENC-only titles | deferred (clear error) |
| Naming templates / Tantivy search / GUI | Phase 2+ |

## Workspace crates

| Crate | Role |
| --- | --- |
| `libation-config` | Settings (TOML + env), paths (`LIBATION_FILES_DIR` / XDG) |
| `libation-audible` | Thin wrapper over `audible-rs` (auth, scan, license/download) |
| `libation-decrypt` | Decrypt pipeline (`aaxclean-cli` v1) |
| `libation-storage` | `StorageBackend` trait: local FS + S3/MinIO |
| `libation-library` | SQLite library DB + migrations (rusqlite, bundled) |
| `libation-liberate` | License → download → decrypt → store |
| `libation-migrate` | Import classic Libation Settings / accounts / DB |
| `libation-search` | Full-text search (Tantivy; Phase 4) |
| `libation-cli` | CLI (`libation`) |
| `libationd` | Daemon: scheduler + HTTP control plane |

## Quick start

```bash
# Build
cargo build --workspace

# Login (SSH/Docker: forward the printed callback port)
export LIBATION_FILES_DIR=./LibationFiles
cargo run -p libation-cli -- auth login -m us

# Sync library, liberate one title (needs aaxclean-cli on PATH)
cargo run -p libation-cli -- library scan
cargo run -p libation-cli -- library liberate --asin B0EXAMPLE

# Migrate from classic Libation Files
cargo run -p libation-cli -- migrate import --from ~/Libation --force

# Daemon
cargo run -p libationd -- --config config/config.example.toml
curl -X POST http://127.0.0.1:8787/scan
curl -X POST http://127.0.0.1:8787/liberate -H 'content-type: application/json' \
  -d '{"asin":"B0EXAMPLE"}'
curl http://127.0.0.1:8787/jobs
```

Decrypt requires [aaxclean-cli](https://github.com/Mbucari/aaxclean-cli) (or set
`AUDIBLE_AAXCLEAN_CLI`).

## Fresh install with existing audiobooks

Point `storage.local.root` (or S3) at your existing library folder, scan, then
match files so liberate will not re-download:

```bash
libation library scan --match-storage
# or later:
libation library set-status
libation library liberate          # skips matched titles
libation library liberate --force  # re-download anyway
```

Matching uses the planned path (`Author/Title/ASIN.ext`) and any path that
contains the ASIN (including classic Libation `Title [ASIN].m4b` names).


```bash
export LIBATION_FILES_DIR=./LibationFiles   # destination for libation-rs
cargo run -p libation-cli -- migrate import --from ~/Libation --force
```

This imports:

- `Settings.json` → `config.toml` (books path, quality, widevine, auto-scan/liberate)
- `AccountsSettings.json` → account rows + audible-rs `.auth` files (when tokens convert)
- `LibationContext.db` → `library.db` (titles, authors, liberate status)
- `FileLocationsV2.json` → `storage_key` paths when present

Use `--dry-run` to preview, `--skip-auth` to import library/metadata without writing auth files.


## License

GPL-3.0-or-later (aligned with upstream Libation). `audible-rs` remains MIT.
