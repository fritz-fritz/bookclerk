//! Minimal naming helper until full template engine (Phase 2).

/// Build a relative storage key for a liberated title.
#[must_use]
pub fn default_storage_key(authors: Option<&str>, title: &str, asin: &str, ext: &str) -> String {
    let author = sanitize(authors.unwrap_or("Unknown Author"));
    let title = sanitize(title);
    let ext = ext.trim_start_matches('.');
    format!("{author}/{title}/{asin}.{ext}")
}

fn sanitize(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => out.push('_'),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    let trimmed = out.trim().trim_matches('.');
    if trimmed.is_empty() {
        "Unknown".into()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_safe_key() {
        let key = default_storage_key(Some("A/B"), "Hello: World", "B00X", "m4b");
        assert_eq!(key, "A_B/Hello_ World/B00X.m4b");
    }
}
