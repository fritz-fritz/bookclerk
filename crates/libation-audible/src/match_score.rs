//! AudioBookshelf-style metadata match confidence scoring.
//!
//! Port of `BookFinder.calculateMatchConfidence` from audiobookshelf
//! (`server/finders/BookFinder.js`), including title/author cleaning and
//! Levenshtein similarity.

use unicode_normalization::UnicodeNormalization;

/// Weights used by AudioBookshelf (duration dominates).
const W_DURATION: f64 = 0.7;
const W_TITLE: f64 = 0.2;
const W_AUTHOR: f64 = 0.1;

/// Candidate metadata used for confidence scoring.
#[derive(Debug, Clone)]
pub struct ScoreInput<'a> {
    pub title: &'a str,
    pub subtitle: Option<&'a str>,
    pub author: Option<&'a str>,
    /// Runtime in minutes (Audible / Audnexus `runtimeLengthMin`).
    pub duration_minutes: Option<f64>,
}

/// Calculate match confidence in `[0.0, 1.0]` (AudioBookshelf algorithm).
///
/// `library_duration_minutes` is the owned title's runtime (e.g. Libro).
/// When `query_title_is_asin` is true, returns `1.0` (exact ASIN lookup).
#[must_use]
pub fn calculate_match_confidence(
    book: &ScoreInput<'_>,
    library_duration_minutes: Option<f64>,
    query_title: &str,
    query_author: Option<&str>,
    query_title_is_asin: bool,
) -> f64 {
    if query_title_is_asin {
        return 1.0;
    }

    let duration_score = match (library_duration_minutes, book.duration_minutes) {
        (Some(lib_mins), Some(book_mins)) => {
            let duration_diff = (book_mins - lib_mins).abs();
            if duration_diff <= 1.0 {
                1.0
            } else if duration_diff <= 5.0 {
                1.1 - 0.1 * duration_diff
            } else if duration_diff <= 10.0 {
                1.2 - 0.12 * duration_diff
            } else {
                0.0
            }
        }
        _ => 0.1,
    };

    let title_query_has_subtitle = has_subtitle(query_title);
    let title_score = title_similarity(query_title, book, title_query_has_subtitle);

    let norm_author_query = clean_author_for_compares(query_author.unwrap_or(""));
    let author_score = if norm_author_query.is_empty() {
        1.0
    } else {
        let norm_book_author = clean_author_for_compares(book.author.unwrap_or(""));
        if norm_book_author.is_empty() {
            0.0
        } else {
            let parts: Vec<String> = norm_book_author
                .split(',')
                .map(|name| name.trim().to_ascii_lowercase())
                .filter(|p| !p.is_empty())
                .collect();
            if parts.is_empty() {
                0.0
            } else {
                let mut max_part_score =
                    levenshtein_similarity(&norm_author_query, &norm_book_author);
                if parts.len() > 1 || norm_book_author.contains(',') {
                    for part in &parts {
                        max_part_score =
                            max_part_score.max(levenshtein_similarity(&norm_author_query, part));
                    }
                }
                max_part_score
            }
        }
    };

    let confidence = W_DURATION * duration_score + W_TITLE * title_score + W_AUTHOR * author_score;
    confidence.clamp(0.0, 1.0)
}

fn title_similarity(title_query: &str, book: &ScoreInput<'_>, keep_subtitle: bool) -> f64 {
    let clean_title = clean_title_for_compares(book.title, keep_subtitle);
    let clean_subtitle = if keep_subtitle {
        book.subtitle
            .filter(|s| !s.is_empty())
            .map(|s| format!(": {s}"))
            .unwrap_or_default()
    } else {
        String::new()
    };
    let norm_book_title = format!("{clean_title}{clean_subtitle}");
    let norm_title_query = clean_title_for_compares(title_query, keep_subtitle);
    levenshtein_similarity(&norm_title_query, &norm_book_title)
}

fn has_subtitle(title: &str) -> bool {
    title.contains(": ") || title.contains(" - ")
}

fn strip_subtitle(title: &str) -> String {
    if let Some((left, _)) = title.split_once(": ") {
        left.trim().to_string()
    } else if let Some((left, _)) = title.split_once(" - ") {
        left.trim().to_string()
    } else {
        title.to_string()
    }
}

fn replace_accented_chars(s: &str) -> String {
    s.nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .collect()
}

fn strip_redundant_spaces(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Clean a title for fuzzy compares (AudioBookshelf `cleanTitleForCompares`).
#[must_use]
pub fn clean_title_for_compares(title: &str, keep_subtitle: bool) -> String {
    if title.is_empty() {
        return String::new();
    }
    let title = strip_redundant_spaces(title);
    let stripped = if keep_subtitle {
        title
    } else {
        strip_subtitle(&title)
    };
    // Remove parenthetical content (ABS: /\([^)]*\)/g).
    let mut without_parens = String::with_capacity(stripped.len());
    let mut chars = stripped.chars();
    while let Some(c) = chars.next() {
        if c == '(' {
            for nc in chars.by_ref() {
                if nc == ')' {
                    break;
                }
            }
        } else {
            without_parens.push(c);
        }
    }
    let cleaned = strip_redundant_spaces(&without_parens).replace('\'', "");
    replace_accented_chars(&cleaned).to_ascii_lowercase()
}

/// Clean an author string for fuzzy compares (AudioBookshelf `cleanAuthorForCompares`).
#[must_use]
pub fn clean_author_for_compares(author: &str) -> String {
    if author.is_empty() {
        return String::new();
    }
    let author = strip_redundant_spaces(author);
    let mut clean = replace_accented_chars(&author).to_ascii_lowercase();
    // Separate initials: "j.k" → "j. k"
    clean = separate_initials(&clean);
    // Remove middle initials: /(?<=\w\w)(\s+[a-z]\.?)+(?=\s+\w\w)/g
    clean = strip_middle_initials(&clean);
    // Remove " et al." / " et al"
    clean = strip_et_al(&clean);
    clean
}

fn separate_initials(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + 4);
    let mut i = 0;
    while i < bytes.len() {
        out.push(bytes[i] as char);
        if (bytes[i] as char).is_ascii_lowercase()
            && i + 2 < bytes.len()
            && bytes[i + 1] == b'.'
            && (bytes[i + 2] as char).is_ascii_lowercase()
        {
            out.push('.');
            out.push(' ');
            i += 2;
            continue;
        }
        i += 1;
    }
    out
}

fn strip_middle_initials(s: &str) -> String {
    let b = s.as_bytes();
    let is_word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0usize;
    while i < b.len() {
        let mut removed = false;
        if i >= 2 && is_word(b[i - 1]) && is_word(b[i - 2]) {
            // Collect end offsets of each `\s+[a-z]\.?` repetition so we can
            // backtrack like JS when the `\s+\w\w` lookahead fails.
            let mut ends = Vec::new();
            let mut j = i;
            loop {
                let before = j;
                while j < b.len() && b[j].is_ascii_whitespace() {
                    j += 1;
                }
                if j == before || j >= b.len() || !b[j].is_ascii_lowercase() {
                    break;
                }
                j += 1;
                if j < b.len() && b[j] == b'.' {
                    j += 1;
                }
                ends.push(j);
            }
            for &end in ends.iter().rev() {
                let mut k = end;
                let before_ws = k;
                while k < b.len() && b[k].is_ascii_whitespace() {
                    k += 1;
                }
                if k > before_ws && k + 1 < b.len() && is_word(b[k]) && is_word(b[k + 1]) {
                    i = end;
                    removed = true;
                    break;
                }
            }
        }
        if !removed {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

fn strip_et_al(s: &str) -> String {
    let lower = s; // already lowercased by caller
    let mut out = String::with_capacity(s.len());
    let bytes = lower.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i..].starts_with(b" et al") {
            let mut j = i + 5;
            if j < bytes.len() && bytes[j] == b'.' {
                j += 1;
            }
            if j == bytes.len() || bytes[j] == b' ' {
                i = j;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

/// Levenshtein distance (case-insensitive by default, matching ABS).
#[must_use]
pub fn levenshtein_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let (m, n) = (a.len(), b.len());
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

/// Similarity in `[0.0, 1.0]` from Levenshtein distance.
#[must_use]
pub fn levenshtein_similarity(a: &str, b: &str) -> f64 {
    let distance = levenshtein_distance(a, b);
    let max_len = a.chars().count().max(b.chars().count());
    if max_len == 0 {
        1.0
    } else {
        1.0 - (distance as f64 / max_len as f64)
    }
}

/// True when `s` looks like an Audible ASIN (`^[A-Z0-9]{10}$`, case-insensitive).
#[must_use]
pub fn is_valid_asin(s: &str) -> bool {
    let s = s.trim();
    s.len() == 10
        && s.chars().all(|c| c.is_ascii_alphanumeric())
        && s.chars().any(|c| c.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_title_author_duration_is_high_confidence() {
        let book = ScoreInput {
            title: "Forward the Foundation",
            subtitle: None,
            author: Some("Isaac Asimov"),
            duration_minutes: Some(970.0),
        };
        let c = calculate_match_confidence(
            &book,
            Some(970.0),
            "Forward the Foundation",
            Some("Isaac Asimov"),
            false,
        );
        assert!(c >= 0.99, "confidence={c}");
    }

    #[test]
    fn large_duration_gap_lowers_score() {
        let book = ScoreInput {
            title: "Forward the Foundation",
            subtitle: None,
            author: Some("Isaac Asimov"),
            duration_minutes: Some(970.0),
        };
        let c = calculate_match_confidence(
            &book,
            Some(200.0),
            "Forward the Foundation",
            Some("Isaac Asimov"),
            false,
        );
        // Duration weight 0.7 → score ≈ 0.3 even with perfect title/author.
        assert!(c < 0.35, "confidence={c}");
        assert!(c >= 0.29, "confidence={c}");
    }

    #[test]
    fn asin_query_is_perfect() {
        let book = ScoreInput {
            title: "Anything",
            subtitle: None,
            author: None,
            duration_minutes: None,
        };
        assert_eq!(
            calculate_match_confidence(&book, None, "B005WWT30E", None, true),
            1.0
        );
    }

    #[test]
    fn cleans_title_parentheses_and_subtitle() {
        assert_eq!(
            clean_title_for_compares("Ender's Game (Ender's Saga)", false),
            "enders game"
        );
        assert_eq!(
            clean_title_for_compares("Cool Book: Coolest Ever", false),
            "cool book"
        );
    }

    #[test]
    fn cleans_author_middle_initials() {
        assert_eq!(
            clean_author_for_compares("John R. R. Tolkien"),
            "john tolkien"
        );
    }
}
