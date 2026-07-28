# paste (Bookclerk path patch)

Vendored sources of [`pastey`](https://github.com/AS1100K/pastey) 0.2.x,
published here under the crate name `paste` / version `1.0.15` so Cargo
`[patch.crates-io]` can replace unmaintained crates.io
[`paste`](https://crates.io/crates/paste)
([RUSTSEC-2024-0436](https://rustsec.org/advisories/RUSTSEC-2024-0436.html)).

The binary linked into Bookclerk is this maintained fork, not the archived
crates.io release. OSV still matches the lockfile *name* `paste`; see
`osv-scanner.toml` for the documented advisory exception.

Upstream: MIT OR Apache-2.0. Do not publish this path crate to crates.io.
Refresh by copying from a current pastey release tag when needed.
