//! Libation-facing audible API: auth, accounts, library sync, download options.
//!
//! Thin wrapper over [audible-rs](https://github.com/mkb79/audible-rs) so Libation
//! keeps stable naming and a `LIBATION_FILES_DIR`-rooted auth layout.

mod accounts;
mod auth;
mod error;
mod options;
mod paths;
mod qr;
mod sync;

pub use accounts::{
    AccountInfo, AccountStatus, import_libation_accounts_json, list_accounts, session_to_info,
};
pub use auth::{AuthLoginOptions, AuthSession, LoginProgress, LoginMode, begin_login};
pub use error::{AudibleError, Result};
pub use options::DownloadOptions;
pub use paths::{auth_dir, auth_file_for, list_auth_files};
pub use qr::{QrRenderMode, render_login_qr};
pub use sync::{ScanOptions, ScanSummary, scan_library};
