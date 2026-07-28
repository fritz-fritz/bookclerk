# Bookclerk patch of audible-rs

Upstream: https://github.com/mkb79/audible-rs @ `5a28f507072022ae7fd7f95a62e3bdc5e515d678`

## Why this tree exists

Bookclerk's library DB uses SeaORM 2 (`sqlx-sqlite`), which depends on
`libsqlite3-sys` `<0.38`. Upstream `audible-rs` pins `rusqlite` 0.40
(`libsqlite3-sys` `^0.38`). Cargo cannot link two `links = "sqlite3"`
crates, even when SeaORM's sqlx driver is unused.

This vendored tree is identical to the pinned upstream revision except
`rusqlite` is lowered to `0.37` so the workspace shares one
`libsqlite3-sys` (`0.35`).

Re-vendor when bumping the `audible-rs` git rev in the workspace
`Cargo.toml`, then re-apply the rusqlite pin.
