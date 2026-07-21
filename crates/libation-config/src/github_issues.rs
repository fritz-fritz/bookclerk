//! GitHub Issues backend for opt-in diagnostics collection.

use serde::Serialize;

use crate::diagnostics::{BufferedEvent, UploadPayload};
use crate::redact::{redact_str, REDACTED};
use crate::settings::DiagnosticsConfig;

/// Environment variables consulted for a GitHub token (never stored in config.toml).
pub const GITHUB_TOKEN_ENV: &str = "LIBATION_DIAGNOSTICS_GITHUB_TOKEN";
pub const GITHUB_TOKEN_FALLBACK_ENV: &str = "GITHUB_TOKEN";

/// Resolve a GitHub token from the environment (diagnostics-specific first).
#[must_use]
pub fn resolve_github_token() -> Option<String> {
    for key in [GITHUB_TOKEN_ENV, GITHUB_TOKEN_FALLBACK_ENV] {
        if let Ok(v) = std::env::var(key) {
            let trimmed = v.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Parse `owner/repo` (or `https://github.com/owner/repo.git`) into `(owner, repo)`.
#[must_use]
pub fn parse_github_repo(raw: &str) -> Option<(String, String)> {
    let s = raw.trim().trim_end_matches('/').trim_end_matches(".git");
    if s.is_empty() {
        return None;
    }
    let s = s
        .strip_prefix("https://github.com/")
        .or_else(|| s.strip_prefix("http://github.com/"))
        .or_else(|| s.strip_prefix("git@github.com:"))
        .unwrap_or(s);
    let mut parts = s.split('/');
    let owner = parts.next()?.trim();
    let repo = parts.next()?.trim();
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

#[derive(Debug, Serialize)]
struct CreateIssueRequest {
    title: String,
    body: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    labels: Vec<String>,
}

/// Build a fully redacted GitHub issue title + body for a diagnostics payload.
#[must_use]
pub fn format_issue(payload: &UploadPayload, events: &[BufferedEvent]) -> (String, String) {
    let title = redact_str(&format!(
        "[diagnostics] {} · libation {} · {}",
        payload.trigger, payload.version, payload.os
    ));

    let mut body = String::new();
    body.push_str("<!-- libation-diagnostics: auto-filed; secrets redacted -->\n");
    body.push_str("## Libation diagnostics report\n\n");
    body.push_str(&format!("- **Trigger:** `{}`\n", payload.trigger));
    body.push_str(&format!("- **Version:** `{}`\n", payload.version));
    body.push_str(&format!("- **OS:** `{}`\n", payload.os));
    body.push_str(&format!(
        "- **Archived at (unix ms):** `{}`\n",
        payload.archived_at_unix_ms
    ));
    body.push_str(&format!("- **Events:** {}\n\n", events.len()));
    body.push_str(
        "This issue was opened automatically from an opt-in `[diagnostics]` upload. \
         Auth tokens, passwords, DRM material, and similar secrets are redacted before upload.\n\n",
    );
    body.push_str("### Recent log events (redacted)\n\n```json\n");
    let json = serde_json::to_string_pretty(events).unwrap_or_else(|_| "[]".into());
    body.push_str(&redact_str(&json));
    body.push_str("\n```\n");

    // GitHub issue body soft limit — keep well under 65536.
    const MAX_BODY: usize = 60_000;
    if body.len() > MAX_BODY {
        body.truncate(MAX_BODY);
        body.push_str("\n\n…truncated…\n");
    }

    (title, redact_str(&body))
}

/// POST a new issue to the configured GitHub repository.
///
/// Returns the issue HTML URL on success.
pub fn create_issue(
    config: &DiagnosticsConfig,
    payload: &UploadPayload,
    events: &[BufferedEvent],
) -> Result<String, String> {
    let token = resolve_github_token().ok_or_else(|| {
        format!("missing GitHub token ({GITHUB_TOKEN_ENV} or {GITHUB_TOKEN_FALLBACK_ENV})")
    })?;
    let (owner, repo) = parse_github_repo(&config.github_repo)
        .ok_or_else(|| format!("invalid diagnostics.github_repo {:?}", config.github_repo))?;

    let (title, body) = format_issue(payload, events);
    // Never allow a leaked token pattern into the issue even if it somehow survived.
    if body.contains(&token) || title.contains(&token) {
        return Err("refusing to open GitHub issue: token appeared in payload".into());
    }

    let api_base = config.github_api_url.trim().trim_end_matches('/');
    let url = format!("{api_base}/repos/{owner}/{repo}/issues");
    let req = CreateIssueRequest {
        title,
        body,
        labels: config.github_labels.clone(),
    };
    let json = serde_json::to_string(&req).map_err(|e| e.to_string())?;
    let json = redact_str(&json);
    if json.contains(&token) {
        return Err("refusing to open GitHub issue: token appeared in request JSON".into());
    }

    let response = ureq::post(&url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28")
        .set(
            "User-Agent",
            &format!("libation-diagnostics/{}", payload.version),
        )
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send_string(&json)
        .map_err(|e| format!("GitHub issue create failed: {e}"))?;

    let status = response.status();
    let resp_body = response.into_string().unwrap_or_default();
    if !(200..300).contains(&status) {
        // Scrub token if the API echoed anything unexpected.
        let safe = resp_body.replace(&token, REDACTED);
        return Err(format!("GitHub API HTTP {status}: {}", redact_str(&safe)));
    }

    let html_url = serde_json::from_str::<serde_json::Value>(&resp_body)
        .ok()
        .and_then(|v| {
            v.get("html_url")
                .and_then(|u| u.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| format!("https://github.com/{owner}/{repo}/issues"));
    Ok(html_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repo_forms() {
        assert_eq!(
            parse_github_repo("fritz-fritz/libation-rs"),
            Some(("fritz-fritz".into(), "libation-rs".into()))
        );
        assert_eq!(
            parse_github_repo("https://github.com/fritz-fritz/libation-rs.git"),
            Some(("fritz-fritz".into(), "libation-rs".into()))
        );
        assert!(parse_github_repo("not-a-repo").is_none());
    }

    #[test]
    fn issue_body_redacts_tokens() {
        let payload = UploadPayload {
            trigger: "crash".into(),
            version: "0.1.0".into(),
            os: "linux".into(),
            archived_at_unix_ms: 1,
            events: vec![],
        };
        let events = vec![BufferedEvent {
            ts_unix_ms: 1,
            level: "ERROR".into(),
            target: "test".into(),
            message: "boom Atna|should-not-appear".into(),
            fields: vec![("refresh_token".into(), "Atnr|nope".into())],
        }];
        // Simulate already-sanitized fields still getting a second pass via format_issue.
        let (_title, body) = format_issue(&payload, &events);
        assert!(!body.contains("Atna|should-not-appear"));
        assert!(!body.contains("Atnr|nope"));
        assert!(body.contains(REDACTED) || body.contains("refresh_token"));
    }
}
