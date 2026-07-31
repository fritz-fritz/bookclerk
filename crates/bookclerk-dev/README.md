# bookclerk-dev

Internal dispatcher for [`.cargo/config.toml`](../../.cargo/config.toml) aliases.
Use these instead of chaining shell scripts manually.

| Command | What it does |
| --- | --- |
| `cargo build-plugins` | Build all first-party guest binaries |
| `cargo stage-plugins` | Build + stage under `target/plugin-artifacts` |
| `cargo dev-daemon` | Build + stage + run `bookclerkd` with external guests |
| `cargo dev-cli` | Build + stage + run `bookclerk` CLI with external guests |
| `cargo test-staged` | Build + stage + run `staged_plugins` handshake test |

Add `--release` to any alias for release builds. Forward host args after `--`:

```bash
cargo dev-daemon -- --help
cargo dev-cli -- version
```

Environment (optional):

- `BOOKCLERK_FILES_DIR` — default `/tmp/BookclerkFiles`
- `BOOKCLERK_PLUGIN_ARTIFACTS` — staging dir (default `target/plugin-artifacts`)

CI continues to call `scripts/*.sh` directly.
