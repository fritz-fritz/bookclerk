# libation-audible

Thin wrapper over [audible-rs](https://github.com/mkb79/audible-rs) for Libation-specific
auth and download options.

## audible-rs pin

```
git = "https://github.com/mkb79/audible-rs"
rev = "5a28f507072022ae7fd7f95a62e3bdc5e515d678"  # v0.1.0-alpha.8 era
```

`libation-library` uses **rusqlite 0.40 with `bundled`**, matching audible-rs, so both can
link a single `libsqlite3-sys` in one binary. Wire the git dependency in the auth-oauth
follow-up.

Until then this crate owns the Libation-facing auth API (QR, callback options,
AccountsSettings import) with stubbed token exchange.
