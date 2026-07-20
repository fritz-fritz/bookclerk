//! Libation-facing audible API: auth, accounts, and download option mapping.
//!
//! `audible-rs` is pinned for the next auth wiring step; `libation-library` now
//! uses rusqlite (same as audible-rs), so the sqlite linker conflict is resolved.

mod accounts;
mod auth;
mod error;
mod options;
mod qr;

pub use accounts::{import_libation_accounts_json, list_accounts_stub, AccountInfo, AccountStatus};
pub use auth::{begin_login, AuthLoginOptions, AuthSession, LoginProgress};
pub use error::{AudibleError, Result};
pub use options::DownloadOptions;
pub use qr::{render_login_qr, QrRenderMode};
