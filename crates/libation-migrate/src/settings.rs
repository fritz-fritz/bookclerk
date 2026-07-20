//! Import classic `Settings.json` into libation-rs [`Config`].

use std::path::Path;

use libation_config::{
    AudioQuality, Config, DownloadFormat, StorageBackendKind,
};
use serde_json::Value;

use crate::error::{MigrateError, Result};

/// Load Settings.json as a JSON object.
pub fn load_settings_json(path: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(path).map_err(|err| {
        MigrateError::Settings(format!("failed to read {}: {err}", path.display()))
    })?;
    serde_json::from_str(&text)
        .map_err(|err| MigrateError::Settings(format!("invalid Settings.json: {err}")))
}

/// Apply classic Settings.json keys onto `config`.
pub fn apply_settings_json(config: &mut Config, settings: &Value) {
    if let Some(books) = string_at(settings, "Books") {
        config.storage.backend = StorageBackendKind::Local;
        config.storage.local.root = Path::new(books).to_path_buf();
    }

    if let Some(quality) = string_at(settings, "FileDownloadQuality") {
        config.download.quality = match quality.to_ascii_lowercase().as_str() {
            "normal" => AudioQuality::Normal,
            _ => AudioQuality::High,
        };
    }

    if let Some(lossy) = bool_at(settings, "DecryptToLossy") {
        config.download.format = if lossy {
            DownloadFormat::Mp3
        } else {
            DownloadFormat::M4b
        };
    }

    if let Some(wv) = bool_at(settings, "UseWidevine") {
        config.download.widevine = wv;
    }
    if let Some(xhe) = bool_at(settings, "Request_xHE_AAC") {
        config.download.xhe_aac = xhe;
    }

    if let Some(folder) = string_at(settings, "FolderTemplate") {
        config.download.folder_template = Some(folder.to_string());
    }
    if let Some(file) = string_at(settings, "FileTemplate") {
        config.download.file_template = Some(file.to_string());
    }

    // AutoDownloadEpisodes is poorly named upstream: it means auto-download
    // after scan (books), not podcasts-only.
    if let Some(auto) = bool_at(settings, "AutoDownloadEpisodes") {
        config.library.auto_liberate = auto;
    }

    if let Some(auto_scan) = bool_at(settings, "AutoScan") {
        // Classic GUI scans about every 5 minutes when enabled.
        config.library.scan_interval_minutes = if auto_scan { 5 } else { 0 };
    }
}

fn string_at<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn bool_at(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(Value::as_bool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_core_settings() {
        let settings = serde_json::json!({
            "Books": "/data/Audiobooks",
            "FileDownloadQuality": "Normal",
            "DecryptToLossy": true,
            "UseWidevine": true,
            "Request_xHE_AAC": true,
            "FolderTemplate": "<author>/<title>",
            "FileTemplate": "<title> [<asin>]",
            "AutoDownloadEpisodes": true,
            "AutoScan": true
        });
        let mut cfg = Config::default();
        apply_settings_json(&mut cfg, &settings);
        assert_eq!(
            cfg.storage.local.root,
            Path::new("/data/Audiobooks")
        );
        assert_eq!(cfg.download.quality, AudioQuality::Normal);
        assert_eq!(cfg.download.format, DownloadFormat::Mp3);
        assert!(cfg.download.widevine);
        assert!(cfg.download.xhe_aac);
        assert_eq!(
            cfg.download.folder_template.as_deref(),
            Some("<author>/<title>")
        );
        assert_eq!(
            cfg.download.file_template.as_deref(),
            Some("<title> [<asin>]")
        );
        assert!(cfg.library.auto_liberate);
        assert_eq!(cfg.library.scan_interval_minutes, 5);
    }
}
