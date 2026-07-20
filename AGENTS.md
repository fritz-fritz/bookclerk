# AGENTS.md

## Cursor Cloud specific instructions

This is a Rust workspace (edition 2021, `rust-toolchain.toml` pins the `stable`
channel with `rustfmt` + `clippy`). The startup update script runs
`cargo fetch`, so dependencies are already downloaded when a session begins.
`ffmpeg` is available on the VM.

### Services / binaries

Two runnable binaries (the workspace `default-members`):

- `libation-cli` (binary `libation`) — headless Audible library manager CLI.
- `libationd` — long-running daemon with an HTTP control plane.

Everything else under `crates/` is a library crate.

### Build / lint / test (mirrors `.github/workflows/ci.yml`)

- Build: `cargo build --workspace`
- Format check: `cargo fmt --all -- --check`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings` (CI treats
  warnings as errors via `RUSTFLAGS="-D warnings"`).
- Test: `cargo test --workspace`
- Release binaries: `cargo build --release -p libation-cli -p libationd`

### Running the apps

Set `LIBATION_FILES_DIR` to a writable dir; on first use the app creates
`library.db` (SQLite, bundled — no external DB needed), plus `cache/`, `logs/`,
and `search_index/` under it.

- CLI: `LIBATION_FILES_DIR=/tmp/LibationFiles cargo run -p libation-cli -- <cmd>`
  (e.g. `version`, `auth list`, `library list`).
- Daemon: `LIBATION_FILES_DIR=/tmp/LibationFiles cargo run -p libationd`.
  It listens on `127.0.0.1:8787` by default (override with
  `LIBATION_DAEMON_LISTEN` or `daemon.listen` in `config.toml`). Control plane:
  `GET /health`, `GET /status`, `POST /scan`, `POST /liberate`, `GET /jobs`.
  `POST` bodies require the `Content-Type: application/json` header (send `{}`
  for defaults), otherwise the request is rejected.

### Non-obvious gotchas

- Actually scanning/liberating a library requires real Audible credentials
  (`libation auth login`). Without a configured account, `scan`/`liberate` jobs
  fail with "no accounts configured" — this is expected, and the daemon +
  control plane still run fine for everything else.
- Optional external tools only needed for real decryption/re-encode work:
  `aaxclean-cli` (Adrm/CENC decrypt; `AUDIBLE_AAXCLEAN_CLI`) and a Widevine
  `.wvd` CDM. Neither is required to build, test, or run the services.
