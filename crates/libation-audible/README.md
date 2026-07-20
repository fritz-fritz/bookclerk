# libation-audible

Thin wrapper over [audible-rs](https://github.com/mkb79/audible-rs) for Libation-specific
auth, library sync, and download options.

## audible-rs pin

```
git = "https://github.com/mkb79/audible-rs"
rev = "5a28f507072022ae7fd7f95a62e3bdc5e515d678"
```

Auth files live under `{LIBATION_FILES_DIR}/auth/*.auth` (audible-rs envelope, plain by
default for headless). `libation-library` uses the same rusqlite 0.40 + bundled SQLite.

## Login modes

- **Server (default):** local reverse-proxy + terminal QR — best for SSH/Docker with port forward
- **External (`--external`):** print authorize URL; paste redirect on stdin or `--response-url`
