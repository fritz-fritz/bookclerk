//! Storage key naming: default layout + classic Libation-style templates.

use std::collections::HashMap;

use libation_config::ReplacementRule;

/// Metadata available to naming templates.
#[derive(Debug, Clone, Default)]
pub struct NamingContext {
    pub asin: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub authors: Option<String>,
    pub narrators: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<String>,
    pub account_id: Option<String>,
    pub publisher: Option<String>,
    pub categories: Option<String>,
    pub length_minutes: Option<i64>,
    pub content_kind: Option<String>,
    /// Chapter-specific: 1-based index.
    pub chapter_number: Option<u32>,
    pub chapter_title: Option<String>,
}

/// Build a relative storage key for a liberated title.
#[must_use]
pub fn default_storage_key(authors: Option<&str>, title: &str, asin: &str, ext: &str) -> String {
    storage_key_with_rules(
        &NamingContext {
            asin: asin.to_string(),
            title: title.to_string(),
            authors: authors.map(str::to_string),
            ..Default::default()
        },
        None,
        None,
        ext,
        &[],
    )
}

/// Build a storage key from optional classic Libation folder/file templates.
#[must_use]
pub fn storage_key(
    ctx: &NamingContext,
    folder_template: Option<&str>,
    file_template: Option<&str>,
    ext: &str,
) -> String {
    storage_key_with_rules(ctx, folder_template, file_template, ext, &[])
}

/// Like [`storage_key`] but applies classic `ReplacementCharacters`.
#[must_use]
pub fn storage_key_with_rules(
    ctx: &NamingContext,
    folder_template: Option<&str>,
    file_template: Option<&str>,
    ext: &str,
    replacement_rules: &[ReplacementRule],
) -> String {
    let folder = expand_template(
        folder_template.unwrap_or("<author>/<title>"),
        ctx,
        replacement_rules,
    );
    let file = expand_template(file_template.unwrap_or("<asin>"), ctx, replacement_rules);
    let ext = ext.trim_start_matches('.');
    let mut parts: Vec<String> = Vec::new();
    for segment in folder.split('/') {
        let cleaned = sanitize_segment(segment, replacement_rules);
        if !cleaned.is_empty() {
            parts.push(cleaned);
        }
    }
    let file = sanitize_segment(&file, replacement_rules);
    let file = if file.is_empty() {
        sanitize_segment(&ctx.asin, replacement_rules)
    } else {
        file
    };
    parts.push(format!("{file}.{ext}"));
    parts.join("/")
}

/// Storage key for a split chapter file.
#[must_use]
pub fn chapter_storage_key(
    ctx: &NamingContext,
    folder_template: Option<&str>,
    chapter_file_template: Option<&str>,
    replacement_rules: &[ReplacementRule],
    chapter_number: usize,
    chapter_title: &str,
    ext: &str,
) -> String {
    let mut ch_ctx = ctx.clone();
    ch_ctx.chapter_number = Some(chapter_number as u32);
    ch_ctx.chapter_title = Some(chapter_title.to_string());
    let template = chapter_file_template.unwrap_or("<ch#> - <chapter title>");
    let folder = expand_template(
        folder_template.unwrap_or("<author>/<title>"),
        &ch_ctx,
        replacement_rules,
    );
    let file = expand_template(template, &ch_ctx, replacement_rules);
    let ext = ext.trim_start_matches('.');
    let folder = folder
        .split('/')
        .map(|s| sanitize_segment(s, replacement_rules))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    let file = sanitize_segment(&file, replacement_rules);
    if folder.is_empty() {
        format!("{file}.{ext}")
    } else {
        format!("{folder}/{file}.{ext}")
    }
}

/// Replace the audio file extension on a storage key (e.g. `.m4b` → `.mp3`).
#[must_use]
pub fn swap_audio_extension(key: &str, new_ext: &str) -> String {
    let ext = new_ext.trim_start_matches('.');
    key.rsplit_once('.')
        .map(|(stem, _)| format!("{stem}.{ext}"))
        .unwrap_or_else(|| format!("{key}.{ext}"))
}

/// Replace the audio file extension on a storage key with a sidecar suffix.
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

fn expand_template(template: &str, ctx: &NamingContext, rules: &[ReplacementRule]) -> String {
    let mut out = expand_conditionals(template, ctx);
    let values = template_values(ctx, rules);
    let mut keys: Vec<_> = values.keys().cloned().collect();
    keys.sort_by_key(|k| std::cmp::Reverse(k.len()));
    for key in keys {
        let value = values.get(&key).map(String::as_str).unwrap_or("");
        let angle = format!("<{key}>");
        let percent = format!("%{key}%");
        out = out.replace(&angle, value);
        out = out.replace(&percent, value);
        if key == "author" {
            out = out.replace("%authors%", value);
        }
        if key == "title" {
            out = out.replace("%fulltitle%", value);
        }
    }
    // Truncation formatters: <title[14]>
    out = apply_truncation_formatters(&out, &values);
    out
}

fn expand_conditionals(template: &str, ctx: &NamingContext) -> String {
    let mut out = template.to_string();
    let flags: [(&str, bool); 4] = [
        ("series", ctx.series.as_ref().is_some_and(|s| !s.is_empty())),
        ("subtitle", ctx.subtitle.as_ref().is_some_and(|s| !s.is_empty())),
        ("narrator", ctx.narrators.as_ref().is_some_and(|s| !s.is_empty())),
        ("categories", ctx.categories.as_ref().is_some_and(|s| !s.is_empty())),
    ];
    for (name, present) in flags {
        let open = format!("<if {name}>");
        let close = "<end if>".to_string();
        while let Some(start) = out.find(&open) {
            let Some(end) = out[start..].find(&close) else {
                break;
            };
            let inner_start = start + open.len();
            let inner_end = start + end;
            let inner = out[inner_start..inner_end].to_string();
            let replacement = if present { inner } else { String::new() };
            out.replace_range(start..inner_end + close.len(), &replacement);
        }
    }
    out
}

fn apply_truncation_formatters(input: &str, values: &HashMap<String, String>) -> String {
    let mut out = input.to_string();
    for (key, value) in values {
        let pattern = format!("<{key}[");
        let mut search_from = 0;
        while let Some(start) = out[search_from..].find(&pattern) {
            let abs_start = search_from + start;
            let Some(bracket) = out[abs_start..].find(']') else {
                break;
            };
            let abs_end = abs_start + bracket;
            let len_str = &out[abs_start + pattern.len()..abs_end];
            let Ok(max_len) = len_str.parse::<usize>() else {
                search_from = abs_end + 1;
                continue;
            };
            let truncated: String = value.chars().take(max_len).collect();
            let token = format!("<{key}[{len_str}]>");
            out = out.replacen(&token, &truncated, 1);
            search_from = abs_start + truncated.len();
        }
    }
    out
}

fn template_values(ctx: &NamingContext, rules: &[ReplacementRule]) -> HashMap<String, String> {
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
    let subtitle = ctx.subtitle.as_deref().unwrap_or("");
    let publisher = ctx.publisher.as_deref().unwrap_or("");
    let categories = ctx.categories.as_deref().unwrap_or("");
    let length = ctx
        .length_minutes
        .map(|m| m.to_string())
        .unwrap_or_default();
    let ch_num = ctx
        .chapter_number
        .map(|n| format!("{n:02}"))
        .unwrap_or_default();
    let ch_title = ctx.chapter_title.as_deref().unwrap_or("");

    let mut map = HashMap::new();
    map.insert("asin".into(), sanitize_segment(&ctx.asin, rules));
    map.insert("title".into(), sanitize_segment(&ctx.title, rules));
    map.insert("subtitle".into(), sanitize_segment(subtitle, rules));
    map.insert("author".into(), sanitize_segment(authors, rules));
    map.insert("first author".into(), sanitize_segment(first_author, rules));
    map.insert("author first".into(), sanitize_segment(first_author, rules));
    map.insert("narrator".into(), sanitize_segment(narrators, rules));
    map.insert("series".into(), sanitize_segment(series, rules));
    map.insert("series#".into(), sanitize_segment(series_index, rules));
    map.insert("account".into(), sanitize_segment(account, rules));
    map.insert("publisher".into(), sanitize_segment(publisher, rules));
    map.insert("categories".into(), sanitize_segment(categories, rules));
    map.insert("length".into(), sanitize_segment(&length, rules));
    map.insert("ch#".into(), sanitize_segment(&ch_num, rules));
    map.insert("chapter title".into(), sanitize_segment(ch_title, rules));
    map.insert("chapter #".into(), sanitize_segment(&ch_num, rules));
    map
}

fn sanitize_segment(input: &str, rules: &[ReplacementRule]) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => out.push('_'),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    let trimmed = out.trim().trim_matches('.');
    if rules.is_empty() {
        libation_config::apply_replacements(trimmed, &libation_config::default_replacement_characters())
    } else {
        libation_config::apply_replacements(trimmed, rules)
    }
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
            ..Default::default()
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
    fn conditional_series_tag() {
        let ctx = NamingContext {
            series: Some("S".into()),
            title: "T".into(),
            asin: "B00X".into(),
            ..Default::default()
        };
        let out = expand_template("<if series>[<series>] <end if><title>", &ctx, &[]);
        assert_eq!(out, "[S] T");
    }
}
