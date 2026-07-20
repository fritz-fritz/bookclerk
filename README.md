# Libation (Rust)

Headless-first Audible library manager — a greenfield Rust rewrite of
[Libation](https://github.com/rmcrackan/Libation), built on
[audible-rs](https://github.com/mkb79/audible-rs).

## Status

Phase 1 in progress: workspace scaffold, rusqlite library DB, audible-rs auth
(login server + external), library scan, CLI/daemon stubs. Liberate pipeline and
S3 multipart uploads come next.

## Workspace crates

| Crate | Role |
| --- | --- |
| `libation-config` | Settings (TOML + env), paths (`LIBATION_FILES_DIR` / XDG) |
| `libation-audible` | Thin wrapper over `audible-rs` (auth, download options) |
| `libation-decrypt` | Decrypt pipeline (`aaxclean-cli` v1) |
| `libation-storage` | `StorageBackend` trait: local FS + S3/MinIO |
| `libation-library` | SQLite library DB + migrations (rusqlite, bundled) |
| `libation-liberate` | License → download → decrypt → metadata → store |
| `libation-search` | Full-text search (Tantivy; Phase 4) |
| `libation-cli` | CLI (`libation`) with Phase 1 verb surface |
| `libationd` | Daemon: scheduler + HTTP control plane |

## Quick start

```bash
# Build
cargo build --workspace

# CLI help
cargo run -p libation-cli -- --help

# Daemon (health on :8787 by default)
cargo run -p libationd -- --config config/config.example.toml
```

## Configuration

Copy [`config/config.example.toml`](config/config.example.toml). Override with
env vars (`LIBATION_*`) or `LIBATION_FILES_DIR` for Libation-compatible data dirs.

## License

GPL-3.0-or-later (aligned with upstream Libation). `audible-rs` remains MIT.
