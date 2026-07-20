# libation-audible

Thin wrapper over [audible-rs](https://github.com/mkb79/audible-rs) for Libation-specific
auth and download options.

## audible-rs pin

```
git = "https://github.com/mkb79/audible-rs"
rev = "5a28f507072022ae7fd7f95a62e3bdc5e515d678"  # v0.1.0-alpha.8 era
```

The crate is **not** a direct Cargo dependency yet: `audible-rs` uses `rusqlite` while
`libation-library` uses `sqlx`, and the two pull incompatible `libsqlite3-sys` versions
(`links = "sqlite3"` conflict). Options for the auth-oauth follow-up:

1. Switch `libation-library` to `rusqlite` (align with audible-rs), or
2. Run audible-rs as a subprocess / separate process, or
3. Contribute / wait for a shared sqlite linker strategy upstream.

Until then this crate owns the Libation-facing auth API (QR, callback options,
AccountsSettings import) with stubbed token exchange.
