//! Configuration, data-directory resolution, and logging setup for Libation.

mod diagnostics;
mod error;
mod extras;
mod github_issues;
mod journal;
mod logging;
mod overrides;
mod paths;
mod redact;
mod settings;

pub use diagnostics::{BufferedEvent, DiagnosticsHandle, UploadPayload};
pub use error::{ConfigError, Result};
pub use extras::{
    apply_replacements, classic_key_aliases, default_replacement_characters,
    key_matches_reconcile_pattern, parse_replacement_characters, posix_replacement_characters,
    reconciliation_wildcard_rules, resolve_replacement_characters, s3_replacement_characters,
    windows_replacement_characters, FileTimestampMode, LameConfig, PathSanitizationMode,
    ReplacementRule, RECONCILE_WILDCARD,
};
pub use github_issues::{parse_github_repo, resolve_github_token, GITHUB_TOKEN_ENV};
pub use journal::journald_available;
pub use logging::{init_tracing, init_tracing_with, LogFormat, LoggingHandle, TracingOptions};
pub use overrides::apply_setting_overrides;
pub use paths::{resolve_files_dir, Paths};
pub use redact::{
    is_sensitive_field, is_upload_identifying_field, redact_field_value, redact_str,
    sanitize_for_remote_upload, REDACTED,
};
pub use settings::{
    AudioQuality, AuthConfig, BadBookAction, Config, DaemonConfig, DiagnosticsBackend,
    DiagnosticsConfig, DownloadConfig, DownloadFormat, LibraryConfig, StorageBackendKind,
    StorageConfig, StorageLocalConfig, StorageS3Config,
};
