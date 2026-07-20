# Libation (Rust)

Headless-first Audible library manager — a greenfield Rust rewrite of
[Libation](https://github.com/rmcrackan/Libation), built on
[audible-rs](https://github.com/mkb79/audible-rs).

## Status

**Phase 1 (headless)** covers the core liberate loop with Adrm **and**
Widevine/CENC, optional mp3 re-encode, xHE-AAC preference, naming templates,
classic Libation Files migrate, CLI, and `libationd`.

Still deferred: full classic template conditionals/formatters, Tantivy search,
PDF/covers sidecars as first-class liberate artifacts, GUI.

### Phase 1 checklist

| Capability | Status |
| --- | --- |
| Auth login (QR / callback server / external paste) | done |
| Auth list / status / import (`.auth` + AccountsSettings.json) | done |
| Migrate classic Libation Files (Settings / accounts / DB / templates) | done |
| Library scan → SQLite (honors `scan_enabled`) | done |
| Liberate Adrm aaxc → aaxclean → store | done |
| Liberate Widevine/CENC (CDM `.wvd`, 000307 fallback) | done |
| Prefer xHE-AAC on Widevine path | done |
| `format=mp3` via ffmpeg re-encode | done |
| Naming templates (`folder_template` / `file_template`) | done |
| Match existing storage media (`set-status` / `--match-storage`) | done |
| Local FS + S3/MinIO storage | done |
| `libationd` scheduled scan + auto-liberate + HTTP control plane | done |
| Tantivy search / GUI | Phase 2+ |

## Quick start

```bash
cargo build --workspace

export LIBATION_FILES_DIR=./LibationFiles
cargo run -p libation-cli -- auth login -m us
cargo run -p libation-cli -- library scan
cargo run -p libation-cli -- library liberate --asin B0EXAMPLE
```

### Tools

| Tool | Needed for |
| --- | --- |
| [aaxclean-cli](https://github.com/Mbucari/aaxclean-cli) | Adrm decrypt; preferred CENC decrypt (`AUDIBLE_AAXCLEAN_CLI`) |
| `ffmpeg` | CENC decrypt fallback; `format=mp3` re-encode (`LIBATION_FFMPEG`) |
| Widevine `.wvd` CDM | Widevine-only titles / `download.widevine=true` |

Place the CDM at `download.widevine_cdm`, `{LIBATION_FILES_DIR}/widevine.wvd`, or
`Accounts/<account>.wvd`.

### Widevine / xHE / mp3

```toml
[download]
widevine = true
xhe_aac = true
format = "mp3"          # requires ffmpeg
widevine_cdm = "device.wvd"
folder_template = "<author>/<title>"
file_template = "<title> [<asin>]"
```

Adrm is tried first when `widevine = false`. If Audible returns `000307` (no
aaxc asset), liberate automatically falls back to Widevine when a CDM is found.

Relative `storage.local.root` values resolve under `LIBATION_FILES_DIR`.

## Fresh install with existing audiobooks

```bash
libation library scan --match-storage
libation library liberate          # skips matched titles
libation library liberate --force  # re-download anyway
```

## Migrate from classic Libation

```bash
export LIBATION_FILES_DIR=./LibationFiles
cargo run -p libation-cli -- migrate import --from ~/Libation --force
```

Imports Settings (including `UseWidevine`, `DecryptToLossy`, `FolderTemplate` /
`FileTemplate`), accounts/auth, and `LibationContext.db`.

## License

GPL-3.0-or-later (aligned with upstream Libation). `audible-rs` remains MIT.
