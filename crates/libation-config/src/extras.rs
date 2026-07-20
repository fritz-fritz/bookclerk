//! Extended Libation classic settings (chapter split, LAME, timestamps, etc.).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Character replacement rule (`ReplacementCharacters` in classic Settings.json).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReplacementRule {
    pub find: String,
    pub replace: String,
}

/// LAME encoder tuning (classic `Lame*` settings).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LameConfig {
    /// `LameTarget`: `quality`, `bitrate`, `constant`, etc.
    pub target: String,
    /// `LameVBRQuality` (0–9, lower = better).
    pub vbr_quality: u8,
    /// `LameBitrate` in kbps when target is bitrate/constant.
    pub bitrate_kbps: u32,
    /// `LameMode`: `default`, `mono`, `stereo`.
    pub mode: String,
    /// `LameDownsampleMono`.
    pub downsample_mono: bool,
    /// `LameConstantBitrate`.
    pub constant_bitrate: bool,
}

impl Default for LameConfig {
    fn default() -> Self {
        Self {
            target: String::from("quality"),
            vbr_quality: 2,
            bitrate_kbps: 128,
            mode: String::from("default"),
            downsample_mono: false,
            constant_bitrate: false,
        }
    }
}

/// Which timestamp to apply after liberate (`CreationTime` / `LastWriteTime`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FileTimestampMode {
    #[default]
    Now,
    Purchased,
    Published,
}

/// Default NTFS-safe replacement map (classic Libation defaults subset).
#[must_use]
pub fn default_replacement_characters() -> Vec<ReplacementRule> {
    [
        (":", "_"),
        ("*", "_"),
        ("?", "_"),
        ("\"", "'"),
        ("<", "("),
        (">", ")"),
        ("|", "_"),
        ("/", "_"),
        ("\\", "_"),
    ]
    .into_iter()
    .map(|(find, replace)| ReplacementRule {
        find: find.into(),
        replace: replace.into(),
    })
    .collect()
}

/// Parse classic `ReplacementCharacters` JSON object.
pub fn parse_replacement_characters(value: &serde_json::Value) -> Vec<ReplacementRule> {
    let Some(map) = value.as_object() else {
        return default_replacement_characters();
    };
    if map.is_empty() {
        return default_replacement_characters();
    }
    map.iter()
        .map(|(k, v)| ReplacementRule {
            find: k.clone(),
            replace: v.as_str().unwrap_or("_").to_string(),
        })
        .collect()
}

/// Apply replacement rules to a string (classic filename sanitization).
#[must_use]
pub fn apply_replacements(input: &str, rules: &[ReplacementRule]) -> String {
    let mut out = input.to_string();
    for rule in rules {
        if !rule.find.is_empty() {
            out = out.replace(&rule.find, &rule.replace);
        }
    }
    out
}

/// Classic Settings.json key → dotted config path for `get-setting` / `-o` overrides.
#[must_use]
pub fn classic_key_aliases() -> HashMap<&'static str, &'static str> {
    HashMap::from([
        ("Books", "storage.local.root"),
        ("FileDownloadQuality", "download.quality"),
        ("DecryptToLossy", "download.format"),
        ("UseWidevine", "download.widevine"),
        ("Request_xHE_AAC", "download.xhe_aac"),
        ("FolderTemplate", "download.folder_template"),
        ("FileTemplate", "download.file_template"),
        ("DownloadCoverArt", "download.download_cover"),
        ("CreateCueSheet", "download.create_cue"),
        ("AllowLibationFixup", "download.fixup_metadata"),
        ("SaveMetadataToFile", "download.save_metadata_json"),
        ("AutoDownloadEpisodes", "library.auto_liberate"),
        ("AutoScan", "library.scan_interval_minutes"),
        ("OverwriteExisting", "download.overwrite_existing"),
        ("InProgress", "download.in_progress"),
        ("ImportEpisodes", "library.import_episodes"),
        ("ImportPlusTitles", "library.import_plus_titles"),
        ("DownloadEpisodes", "library.download_episodes"),
        ("BadBook", "download.bad_book_action"),
        ("SplitFilesByChapter", "download.split_files_by_chapter"),
        ("ChapterFileTemplate", "download.chapter_file_template"),
        ("ChapterTitleTemplate", "download.chapter_title_template"),
        ("MinimumFileDuration", "download.minimum_file_duration_minutes"),
        ("CombineNestedChapterTitles", "download.combine_nested_chapter_titles"),
        ("MergeOpeningAndEndCredits", "download.merge_opening_and_end_credits"),
        ("StripUnabridged", "download.strip_unabridged"),
        ("StripAudibleBrandAudio", "download.strip_audible_brand_audio"),
        ("DownloadClipsBookmarks", "download.download_clips_bookmarks"),
        ("RetainAaxFile", "download.retain_aax_file"),
        ("DownloadSpeedLimit", "download.download_speed_limit_kbps"),
        ("LameTarget", "download.lame.target"),
        ("LameVBRQuality", "download.lame.vbr_quality"),
        ("LameBitrate", "download.lame.bitrate_kbps"),
        ("LameMode", "download.lame.mode"),
        ("LameDownsampleMono", "download.lame.downsample_mono"),
        ("LameConstantBitrate", "download.lame.constant_bitrate"),
        ("MaxSampleRate", "download.max_sample_rate"),
        ("CreationTime", "download.creation_time"),
        ("LastWriteTime", "download.last_write_time"),
    ])
}
