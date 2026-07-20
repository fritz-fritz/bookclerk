//! Libation-facing audible API: auth, accounts, library sync, download options.
//!
//! Thin wrapper over [audible-rs](https://github.com/mkb79/audible-rs) so Libation
//! keeps stable naming and a `LIBATION_FILES_DIR`-rooted auth layout.

mod accounts;
mod artifacts;
mod auth;
mod download;
mod error;
mod options;
mod paths;
mod qr;
mod sync;
mod widevine;

pub use accounts::{
    import_auth_file, import_libation_accounts_json, list_accounts, resolve_auth_file,
    resolve_auth_file_async, session_to_info, AccountInfo, AccountStatus,
};
pub use artifacts::{
    download_companion_pdf, download_cover_jpeg, fetch_chapter_info,
};
pub use auth::{AuthLoginOptions, AuthSession, LoginProgress, LoginMode, begin_login};
pub use download::{
    download_licensed_audio, fetch_and_download, fetch_and_download_with_options,
    open_account_client, request_content_license, summarize_license, AccountClient, DrmKind,
    EncryptedDownload, LicenseSummary,
};
pub use error::{AudibleError, Result};
pub use options::DownloadOptions;
pub use paths::{auth_dir, auth_file_for, list_auth_files};
pub use qr::{QrRenderMode, render_login_qr};
pub use sync::{
    scan_account_into_library, scan_library, ScanOptions, ScanSummary,
};
pub use widevine::{load_widevine_cdm, WidevineCdm, WidevineDownload};
