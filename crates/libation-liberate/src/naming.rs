//! Storage key naming: default layout + classic Libation-style templates.

use std::collections::HashMap;

/// Metadata available to naming templates.
#[derive(Debug, Clone, Default)]
pub struct NamingContext {
    pub asin: String,
    pub title: String,
    pub authors: Option<String>,
    pub narrators: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<String>,
    pub account_id: Option<String>,
}

/// Build a relative storage key for a liberated title.
#[must_use]
pub fn default_storage_key(authors: Option<&str>, title: &str, asin: &str, ext: &str) -> String {
    storage_key(
        &NamingContext {
            asin: asin.to_string(),
            title: title.to_string(),
            authors: authors.map(str::to_string),
            ..Default::default()
        },
        None,
        None,
        ext,
    )
}

/// Build a storage key from optional classic Libation folder/file templates.
///
/// Supported tags (angle-bracket classic style and `%var%` audible-rs style):
/// `asin`, `title`, `author`, `first author` / `author first`, `narrator`,
/// `series`, `series#`, `account`.
#[must_use]
pub fn storage_key(
    ctx: &NamingContext,
    folder_template: Option<&str>,
    file_template: Option<&str>,
    ext: &str,
) -> String {
    let folder = expand_template(
        folder_template.unwrap_or("<author>/<title>"),
        ctx,
    );
    let file = expand_template(file_template.unwrap_or("<asin>"), ctx);
    let ext = ext.trim_start_matches('.');
    let mut parts: Vec<String> = Vec::new();
    for segment in folder.split('/') {
        let cleaned = sanitize_segment(segment);
        if !cleaned.is_empty() {
            parts.push(cleaned);
        }
    }
    let file = sanitize_segment(&file);
    let file = if file.is_empty() {
        sanitize_segment(&ctx.asin)
    } else {
        file
    };
    parts.push(format!("{file}.{ext}"));
    parts.join("/")
}

/// Replace the audio file extension on a storage key with a sidecar suffix.
///
/// Example: `Author/Title/B00X.m4b` + `"pdf"` → `Author/Title/B00X.pdf`;
/// suffix may include dots, e.g. `chapters.tree.json`.
#[must_use]
pub fn sidecar_key(audio_storage_key: &str, suffix: &str) -> String {
    let base = audio_storage_key
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .unwrap_or(audio_storage_key);
    format!("{base}.{suffix}")
}

/// Basename of the liberated audio file (for `.cue` `FILE` lines).
#[must_use]
pub fn audio_basename(storage_key: &str) -> String {
    storage_key
        .rsplit('/')
        .next()
        .unwrap_or(storage_key)
        .to_string()
}

fn expand_template(template: &str, ctx: &NamingContext) -> String {
    let values = template_values(ctx);
    let mut out = template.to_string();

    // Longer keys first so `<first author>` wins over `<author>`.
    let mut keys: Vec<_> = values.keys().cloned().collect();
    keys.sort_by_key(|k| std::cmp::Reverse(k.len()));

    for key in keys {
        let value = values.get(&key).map(String::as_str).unwrap_or("");
        let angle = format!("<{key}>");
        let percent = format!("%{key}%");
        out = out.replace(&angle, value);
        out = out.replace(&percent, value);
        // audible-rs aliases
        if key == "author" {
            out = out.replace("%authors%", value);
        }
        if key == "title" {
            out = out.replace("%fulltitle%", value);
        }
    }
    out
}

fn template_values(ctx: &NamingContext) -> HashMap<String, String> {
    let first_author = ctx
        .authors
        .as_deref()
        .and_then(|a| a.split(',').next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Unknown Author");
    let authors = ctx.authors.as_deref().unwrap_or("Unknown Author");
    let narrators = ctx.narrators.as_deref().unwrap_or("");
    let series = ctx.series.as_deref().unwrap_or("");
    let series_index = ctx.series_index.as_deref().unwrap_or("");
    let account = ctx.account_id.as_deref().unwrap_or("");

    let mut map = HashMap::new();
    // Sanitize values (not template slashes) so author "A/B" becomes "A_B".
    map.insert("asin".into(), sanitize_segment(&ctx.asin));
    map.insert("title".into(), sanitize_segment(&ctx.title));
    map.insert("author".into(), sanitize_segment(authors));
    map.insert("first author".into(), sanitize_segment(first_author));
    map.insert("author first".into(), sanitize_segment(first_author));
    map.insert("narrator".into(), sanitize_segment(narrators));
    map.insert("series".into(), sanitize_segment(series));
    map.insert("series#".into(), sanitize_segment(series_index));
    map.insert("account".into(), sanitize_segment(account));
    map
}

fn sanitize_segment(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => out.push('_'),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    let trimmed = out.trim().trim_matches('.');
    trimmed.to_string()
}

fn sanitize(input: &str) -> String {
    let s = sanitize_segment(input);
    if s.is_empty() {
        "Unknown".into()
    } else {
        s
    }
}

// Keep sanitize used by tests / default path parity.
#[allow(dead_code)]
fn _sanitize_compat(authors: Option<&str>, title: &str) -> (String, String) {
    (sanitize(authors.unwrap_or("Unknown Author")), sanitize(title))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_key_replaces_audio_extension() {
        assert_eq!(
            sidecar_key("Author/Title/B00X.m4b", "pdf"),
            "Author/Title/B00X.pdf"
        );
        assert_eq!(
            sidecar_key("Author/Title/B00X.mp3", "chapters.tree.json"),
            "Author/Title/B00X.chapters.tree.json"
        );
    }

    #[test]
    fn builds_safe_key() {
        let key = default_storage_key(Some("A/B"), "Hello: World", "B00X", "m4b");
        assert_eq!(key, "A_B/Hello_ World/B00X.m4b");
    }

    #[test]
    fn classic_templates() {
        let ctx = NamingContext {
            asin: "B00EXAMPLE1".into(),
            title: "Cool Book".into(),
            authors: Some("Jane Doe, John Smith".into()),
            narrators: Some("Reader".into()),
            series: Some("Cool Series".into()),
            series_index: Some("2".into()),
            account_id: None,
        };
        let key = storage_key(
            &ctx,
            Some("<author>/<title>"),
            Some("<title> [<asin>]"),
            "m4b",
        );
        assert_eq!(key, "Jane Doe, John Smith/Cool Book/Cool Book [B00EXAMPLE1].m4b");
    }

    #[test]
    fn percent_style_templates() {
        let ctx = NamingContext {
            asin: "B00X".into(),
            title: "T".into(),
            authors: Some("A".into()),
            ..Default::default()
        };
        let key = storage_key(&ctx, Some("%author%/%title%"), Some("%asin%"), "mp3");
        assert_eq!(key, "A/T/B00X.mp3");
    }

    #[test]
    fn empty_folder_segments_collapsed() {
        let ctx = NamingContext {
            asin: "B00X".into(),
            title: "T".into(),
            authors: None,
            series: None,
            ..Default::default()
        };
        let key = storage_key(&ctx, Some("<series>/<author>"), Some("<asin>"), "m4b");
        assert_eq!(key, "Unknown Author/B00X.m4b");
    }
}
