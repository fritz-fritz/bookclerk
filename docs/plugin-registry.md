# Plugin registry (crates.io taxonomy + install without Rust)

Third-party plugins remain **prebuilt executables** + `plugin.toml` (see
[plugins.md](plugins.md)). This document defines how publishers advertise them
on [crates.io](https://crates.io) so Bookclerk can **discover** and **install**
them from a packaged binary — without the operator having a Rust toolchain.

> Status: **taxonomy + metadata contract are stable to implement against**.
> Catalog search / one-click install / dashboard browser are phased (below).

## Design principle

| Layer | Role |
| --- | --- |
| crates.io crate | **Discovery index** + source for plugin *authors* |
| GitHub Releases (or equivalent) | **Prebuilt binaries** for each OS/arch |
| `$BOOKCLERK_FILES_DIR/plugins/<id>/` | **Installed** layout Bookclerk already loads |

Bookclerk never runs `cargo build` on the user’s machine. Installing a plugin
downloads a release asset, verifies it, and unpacks `plugin.toml` + binary —
the same layout as a manual drop-in.

```text
crates.io ──search / metadata──► bookclerk plugins search|install
                                        │
                                        ▼
                              GitHub Release asset
                              (linux-x86_64, …)
                                        │
                                        ▼
                              plugins/<id>/{plugin.toml, binary}
```

## Crate naming taxonomy

Publish under a **predictable crate name**:

```text
bookclerk-plugin-{kind}-{id}
```

| Segment | Rules | Examples |
| --- | --- | --- |
| Prefix | Always `bookclerk-plugin-` | — |
| `{kind}` | One of `source`, `integration`, `output`, `database` | `source` |
| `{id}` | Plugin id: lowercase `a-z`, `0-9`, `_` only; 2–32 chars; must match `plugin.toml` `id` and `[sources.<id>]` / … | `spotify`, `my_store` |

**Examples**

| Crate | Kind | Plugin id |
| --- | --- | --- |
| `bookclerk-plugin-source-spotify` | source | `spotify` |
| `bookclerk-plugin-integration-plex` | integration | `plex` |
| `bookclerk-plugin-output-webdav` | output | `webdav` |
| `bookclerk-plugin-database-libsql` | database | `libsql` |

**Binary name:** prefer the same string as the crate name
(`bookclerk-plugin-source-spotify`), referenced from `plugin.toml` `command`.

**Do not** publish first-party in-process adapters under this prefix (Audible,
Libro.fm, … stay workspace crates). The echo example keeps `publish = false`
until it is intentionally released as a template.

### crates.io keywords & categories

Every published plugin crate **must** include:

```toml
[package]
name = "bookclerk-plugin-source-example"
keywords = ["bookclerk", "bookclerk-plugin", "audiobook"]
# optional kind keyword for filtering:
# keywords = ["bookclerk", "bookclerk-plugin", "bookclerk-source", "audiobook"]
categories = ["multimedia", "command-line-utilities"]
```

Kind-specific keyword (optional but recommended): `bookclerk-source`,
`bookclerk-integration`, `bookclerk-output`, `bookclerk-database`.

Discovery queries:

1. Prefer crates whose name starts with `bookclerk-plugin-`
2. And/or keyword `bookclerk-plugin`
3. Then parse `{kind}` / `{id}` from the name (authoritative over loose keywords)

## Cargo package metadata

Authors declare install metadata in `Cargo.toml` so the host does not need a
git clone to know where binaries live:

```toml
[package.metadata.bookclerk]
api_version = 1
kind = "source"                 # must match name segment
id = "example"                  # must match name segment + plugin.toml
display_name = "Example Store"
# Prebuilt assets (see artifact naming below). {tag} {target} {crate} substituted.
artifact_base_url = "https://github.com/example/bookclerk-plugin-source-example/releases/download/{tag}"
# Optional: path inside the archive to the plugin root (default: ".")
# archive_root = "."
# Optional: min Bookclerk host version (semver req), when enforced
# min_host = "0.1.0"
```

The crate’s `readme` / crate description should summarize trust/scope (what
accounts it talks to). Enabling still means running that binary — see the
trust model in [plugins.md](plugins.md).

## Artifact naming (install without Rust)

Release each version as a **per-target archive** (`.tar.gz` on Unix, `.zip` on
Windows) containing at least:

```text
plugin.toml
bookclerk-plugin-{kind}-{id}    # or .exe on Windows
```

Recommended asset names (cargo-dist / cargo-binstall friendly):

```text
{crate}-{version}-{target}.tar.gz
{crate}-{version}-{target}.zip
```

Examples:

```text
bookclerk-plugin-source-example-0.1.0-x86_64-unknown-linux-gnu.tar.gz
bookclerk-plugin-source-example-0.1.0-aarch64-apple-darwin.tar.gz
bookclerk-plugin-source-example-0.1.0-x86_64-pc-windows-msvc.zip
```

`artifact_base_url` + naming → full URL, e.g.:

```text
{artifact_base_url}/{crate}-{version}-{target}.tar.gz
```

with `{tag}` usually `v{version}`.

**Checksums:** publish `SHA256SUMS` (or per-asset `.sha256`) next to assets.
Future install will require a matching digest before enabling by default.

**Signing (later):** optional minisign/cosign; dashboard can surface “signed by
publisher” vs “crates.io metadata only”.

## Host install layout

`bookclerk plugins install <crate-or-id>` (planned) should:

1. Resolve crate via crates.io (name taxonomy or exact crate)
2. Read `[package.metadata.bookclerk]` from the crate’s published Cargo.toml
3. Pick the host `target_triple` (or override)
4. Download + verify the archive
5. Extract into `$BOOKCLERK_FILES_DIR/plugins/<id>/`
6. Leave `enabled = false` for integrations (existing default); operator enables
   in config / dashboard

No `cargo` / `rustc` on PATH is required.

## Operator dashboard (phased)

| Phase | What |
| --- | --- |
| **A — Taxonomy (this doc + types)** | Stable crate names, metadata schema, validation helpers |
| **B — CLI catalog** | `bookclerk plugins search [query]` against crates.io; `info` shows registry metadata |
| **C — CLI install** | Download/verify/unpack; `plugins update` / `remove` |
| **D — Daemon API** | Authenticated `/api/plugins/catalog`, `/api/plugins/install` |
| **E — Dashboard browser** | Browse / configure / enable in the operator SPA |

Phases D–E reuse the same catalog DTOs as the CLI. Config knobs remain in
`config.toml` tables; the UI edits those settings after install.

## Curated index (optional later)

crates.io is a convenient global index but is not a security boundary. A
future **Bookclerk plugin index** (signed JSON, hosted by the project) can:

- Mirror only reviewed crates
- Pin artifact URLs + digests
- Power the dashboard “Featured” list

The naming taxonomy stays identical so community crates remain discoverable
even when not featured.

## Non-goals

- Compiling plugins from crates.io source on the operator machine
- Loading `cdylib` / WASM into the host process (subprocess RPC stays the ABI)
- Requiring plugin authors to use Rust (any language that ships the archive +
  speaks JSON-RPC is fine; crates.io is optional for non-Rust publishers — they
  can still ship archives and be listed on a curated index)

Non-Rust publishers: use the same `plugin.toml` + asset naming; omit the crate
or publish a thin “manifest-only” crate that only carries
`[package.metadata.bookclerk]` and documentation.

## Validation checklist for publishers

- [ ] Crate name = `bookclerk-plugin-{kind}-{id}`
- [ ] `keywords` include `bookclerk` and `bookclerk-plugin`
- [ ] `[package.metadata.bookclerk]` `kind` / `id` / `api_version` match the name and `plugin.toml`
- [ ] Release assets for each supported target with checksums
- [ ] `plugin.toml` `command` points at the binary inside the archive
- [ ] Document required config keys and any password env vars

## Related

- Runtime discovery & protocol: [plugins.md](plugins.md)
- Architecture overview: [architecture.md](architecture.md)
- Operator GUI surfaces: [gui.md](gui.md) (plugin browser = future)
