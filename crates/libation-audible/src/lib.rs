//! Libation-facing audible API: auth, accounts, library sync, download options.
//!
//! Thin wrapper over [audible-rs](https://github.com/mkb79/audible-rs) so Libation
//! keeps stable naming and a `LIBATION_FILES_DIR`-rooted Accounts layout.

mod accounts;
mod artifacts;
mod auth;
mod download;
mod enrich;
mod error;
mod match_score;
mod options;
mod paths;
mod public_meta;
mod qr;
mod secret;
mod source;
mod sync;
mod throttle;
mod widevine;

pub use accounts::{
    import_auth_file, import_auth_file_with_options, import_libation_accounts_json,
    import_mkb79_auth_json, list_accounts, resolve_auth_file, resolve_auth_file_async,
    session_to_info, AccountInfo, AccountStatus,
};
pub use artifacts::{
    download_companion_pdf, download_cover_jpeg, fetch_chapter_info, fetch_clips_bookmarks,
    fetch_product_metadata,
};
pub use audible_rs::models::content::DownloadLicense;
pub use auth::{
    begin_login, load_authenticator, save_authenticator, AuthLoginOptions, AuthSession, LoginMode,
    LoginProgress, SaveAuthOptions,
};
pub use download::{
    download_licensed_audio, fetch_and_download, fetch_and_download_with_options,
    license_full_json, open_account_client, parse_license_json, request_content_license,
    summarize_license, AccountClient, DrmKind, EncryptedDownload, LicenseSummary,
};
pub use enrich::{
    apply_enrichment_to_book, confidence_percent_to_fraction, enrich_libro_books_from_audible,
    lookup_by_metadata, lookup_by_metadata_with_client, Enrichment, ScoredMatch,
    DEFAULT_ENRICH_MIN_CONFIDENCE,
};
pub use error::{AudibleError, Result};
pub use libation_source::{ScanOptions, ScanSummary};
pub use match_score::{
    calculate_match_confidence, clean_author_for_compares, clean_title_for_compares, is_valid_asin,
    isbn_exact_match, levenshtein_distance, levenshtein_similarity, normalize_isbn, MatchQuery,
    ScoreInput,
};
pub use options::DownloadOptions;
pub use paths::{
    accounts_dir, auth_file_for, ensure_accounts_dir, list_auth_files, widevine_cdm_file_for,
};
pub use public_meta::{
    fetch_audnexus_book, fetch_audnexus_chapters, fetch_public_chapter_info, normalize_region,
    public_http_client, region_tld, search_catalog_asins, search_catalog_keywords,
};
pub use qr::{render_login_qr, QrRenderMode};
pub use secret::{
    configure_auth_secrets, default_allow_plaintext, read_or_create_password_file,
    require_auth_password, resolve_auth_password, AUTH_PASSWORD_ENV, AUTH_PASSWORD_FILE_ENV,
};
pub use source::AudibleSource;
pub use sync::{scan_account_into_library, scan_library};
pub use widevine::{
    effective_cdm_provider, ensure_widevine_cdm, load_widevine_cdm, WidevineCdm, WidevineDownload,
    DEFAULT_WIDEVINE_CDM_PROVIDER,
};
