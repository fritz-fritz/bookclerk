//! Netflix-style Discover shelves built from scored storefront candidates.

use std::collections::{HashMap, HashSet};

use crate::recommend::Recommendation;

/// One horizontal Discover row (series to finish, more by author, …).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiscoverShelf {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub items: Vec<Recommendation>,
}

/// Full Discover page payload: ordered shelves of personalized candidates.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct DiscoverFeed {
    pub shelves: Vec<DiscoverShelf>,
}

/// Taste context used to title and fill personalized shelves.
#[derive(Debug, Clone, Default)]
pub struct ShelfTaste {
    /// Lowercase author → (display name, weight).
    pub authors: HashMap<String, (String, f64)>,
    /// Lowercase narrator → (display name, weight).
    pub narrators: HashMap<String, (String, f64)>,
    /// Lowercase category/subject → (display name, weight).
    pub categories: HashMap<String, (String, f64)>,
    /// Seed title (lowercase) → authors on that seed (display).
    pub seed_authors_by_title: HashMap<String, Vec<String>>,
}

const SHELF_CAP: usize = 12;
const AUTHOR_SHELVES: usize = 3;
const BECAUSE_SHELVES: usize = 2;

/// Partition scored recommendations into Discover shelves (items may repeat).
#[must_use]
pub fn build_discover_feed(
    recs: &[Recommendation],
    taste: &ShelfTaste,
    per_shelf: usize,
) -> DiscoverFeed {
    let cap = per_shelf.clamp(4, SHELF_CAP);
    let mut shelves = Vec::new();

    push_shelf(
        &mut shelves,
        "finish_series",
        "Finish these series",
        Some("Gaps in series you already own"),
        filter_sorted(recs, |r| {
            r.reasons
                .iter()
                .any(|x| x.contains("complete series") || x.contains("next book in series"))
        }),
        cap,
    );

    push_shelf(
        &mut shelves,
        "keep_listening",
        "Pick up where you left off",
        Some("Series you’re actively listening to"),
        filter_sorted(recs, |r| {
            r.reasons.iter().any(|x| {
                x.contains("actively being listened")
                    || x.contains("multiple books")
                    || x.contains("listening activity in")
            })
        }),
        cap,
    );

    // More from top liked authors.
    let mut authors: Vec<_> = taste.authors.values().cloned().collect();
    authors.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    for (display, _) in authors.into_iter().take(AUTHOR_SHELVES) {
        let key = display.to_lowercase();
        let id = format!("author:{}", slugish(&key));
        let title = format!("More from {display}");
        push_shelf(
            &mut shelves,
            &id,
            &title,
            Some("Because you’ve liked their work"),
            filter_sorted(recs, |r| {
                r.authors
                    .as_deref()
                    .map(|a| {
                        split_people(a)
                            .iter()
                            .any(|n| n.eq_ignore_ascii_case(&display))
                    })
                    .unwrap_or(false)
            }),
            cap,
        );
    }

    // “If you like X, try these” — related/similar hits that are not by X.
    let mut because_authors: Vec<_> = taste.authors.values().cloned().collect();
    because_authors.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    for (display, _) in because_authors.into_iter().take(BECAUSE_SHELVES) {
        let id = format!("because:{}", slugish(&display.to_lowercase()));
        let title = format!("If you like {display}");
        push_shelf(
            &mut shelves,
            &id,
            &title,
            Some("Related titles and similar tastes — new voices"),
            filter_sorted(recs, |r| because_you_like(r, &display, taste)),
            cap,
        );
    }

    // Narrators you love.
    let mut narrators: Vec<_> = taste.narrators.values().cloned().collect();
    narrators.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    if let Some((display, _)) = narrators.into_iter().next() {
        let id = format!("narrator:{}", slugish(&display.to_lowercase()));
        push_shelf(
            &mut shelves,
            &id,
            &format!("Narrated by {display}"),
            Some("More from a narrator you enjoy"),
            filter_sorted(recs, |r| {
                r.narrators
                    .as_deref()
                    .map(|n| {
                        split_people(n)
                            .iter()
                            .any(|x| x.eq_ignore_ascii_case(&display))
                    })
                    .unwrap_or(false)
                    || r.reasons.iter().any(|x| {
                        x.to_lowercase()
                            .contains(&format!("liked narrator ({})", display.to_lowercase()))
                    })
            }),
            cap,
        );
    }

    push_shelf(
        &mut shelves,
        "similar_taste",
        "Similar to books you finish",
        Some("Embedding / taste overlap"),
        filter_sorted(recs, |r| {
            r.reasons
                .iter()
                .any(|x| x.contains("similar to titles you finish"))
        }),
        cap,
    );

    push_shelf(
        &mut shelves,
        "requests",
        "Your requests",
        Some("Open title requests"),
        filter_sorted(recs, |r| r.from_request),
        cap,
    );

    // Drop empty shelves; keep at least a "Top picks" dump if everything empty but recs exist.
    shelves.retain(|s| !s.items.is_empty());
    if shelves.is_empty() && !recs.is_empty() {
        let mut top = recs.to_vec();
        top.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        top.truncate(cap);
        shelves.push(DiscoverShelf {
            id: String::from("top_picks"),
            title: String::from("Top picks for you"),
            subtitle: Some(String::from("Best overall matches right now")),
            items: top,
        });
    }

    DiscoverFeed { shelves }
}

fn because_you_like(r: &Recommendation, liked_author: &str, taste: &ShelfTaste) -> bool {
    if r.from_request {
        return false;
    }
    // Must not be another book by the same liked author.
    if r.authors
        .as_deref()
        .map(|a| {
            split_people(a)
                .iter()
                .any(|n| n.eq_ignore_ascii_case(liked_author))
        })
        .unwrap_or(false)
    {
        return false;
    }

    let liked = liked_author.to_lowercase();
    // Seeded from a title by that author (related / series expand).
    let from_their_seed = r.reasons.iter().any(|origin_line| {
        // origins look like `libro related to “Title”` — match seed title map.
        for (seed_title, authors) in &taste.seed_authors_by_title {
            if !authors.iter().any(|a| a.eq_ignore_ascii_case(liked_author)) {
                continue;
            }
            if origin_line.to_lowercase().contains(seed_title) {
                return true;
            }
        }
        false
    });
    if from_their_seed {
        return true;
    }

    // Strong similarity / storefront related without being the same author.
    let similar = r
        .reasons
        .iter()
        .any(|x| x.contains("similar to titles you finish") || x.contains("related"));
    if similar {
        // Prefer when overall taste includes this author heavily — already gated by shelf.
        return !liked.is_empty();
    }
    false
}

fn filter_sorted(
    recs: &[Recommendation],
    pred: impl Fn(&Recommendation) -> bool,
) -> Vec<Recommendation> {
    let mut out: Vec<_> = recs.iter().filter(|r| pred(r)).cloned().collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

fn push_shelf(
    shelves: &mut Vec<DiscoverShelf>,
    id: &str,
    title: &str,
    subtitle: Option<&str>,
    mut items: Vec<Recommendation>,
    cap: usize,
) {
    items.truncate(cap);
    // De-dupe within shelf by product key.
    let mut seen = HashSet::new();
    items.retain(|r| {
        let key = r
            .asin
            .as_deref()
            .map(|a| format!("asin:{}", a.to_ascii_uppercase()))
            .or_else(|| r.isbn.as_deref().map(|i| format!("isbn:{i}")))
            .or_else(|| {
                r.candidate_product_id
                    .as_ref()
                    .map(|p| format!("{}:{}", r.candidate_source.as_deref().unwrap_or("?"), p))
            })
            .unwrap_or_else(|| format!("title:{}", r.title.to_lowercase()));
        seen.insert(key)
    });
    if items.is_empty() {
        return;
    }
    shelves.push(DiscoverShelf {
        id: id.to_string(),
        title: title.to_string(),
        subtitle: subtitle.map(str::to_string),
        items,
    });
}

fn split_people(s: &str) -> Vec<String> {
    s.split([',', ';', '&', '/'])
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect()
}

fn slugish(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Flatten shelves into a unique score-sorted list (CLI / legacy).
#[must_use]
pub fn flatten_feed(feed: &DiscoverFeed, limit: usize) -> Vec<Recommendation> {
    let mut by_key: HashMap<String, Recommendation> = HashMap::new();
    for shelf in &feed.shelves {
        for r in &shelf.items {
            let key = r
                .asin
                .as_deref()
                .map(|a| format!("asin:{}", a.to_ascii_uppercase()))
                .or_else(|| r.isbn.as_deref().map(|i| format!("isbn:{i}")))
                .or_else(|| {
                    r.candidate_product_id
                        .as_ref()
                        .map(|p| format!("{}:{}", r.candidate_source.as_deref().unwrap_or("?"), p))
                })
                .unwrap_or_else(|| format!("title:{}", r.title.to_lowercase()));
            by_key
                .entry(key)
                .and_modify(|e| {
                    if r.score > e.score {
                        *e = r.clone();
                    }
                })
                .or_insert_with(|| r.clone());
        }
    }
    let mut out: Vec<_> = by_key.into_values().collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(limit);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(title: &str, score: f64, reasons: &[&str]) -> Recommendation {
        Recommendation {
            work_id: None,
            title: title.into(),
            authors: Some("Ada Author".into()),
            narrators: None,
            series: Some("Test Series".into()),
            series_index: Some("3".into()),
            asin: Some(title.into()),
            isbn: None,
            score,
            reasons: reasons.iter().map(|s| (*s).to_string()).collect(),
            purchase_hints: Vec::new(),
            from_request: false,
            request_uuid: None,
            candidate_source: Some("audible".into()),
            candidate_product_id: Some(title.into()),
        }
    }

    #[test]
    fn builds_finish_and_because_shelves() {
        let recs = vec![
            rec(
                "Book3",
                30.0,
                &[
                    "complete series (“Test Series”; own 2)",
                    "next book in series (after 2)",
                ],
            ),
            Recommendation {
                authors: Some("Other Writer".into()),
                reasons: vec![
                    "libro related to “Seed By Ada”".into(),
                    "similar to titles you finish (sim 0.40)".into(),
                ],
                ..rec("OtherBook", 18.0, &[])
            },
        ];
        let mut taste = ShelfTaste::default();
        taste
            .authors
            .insert("ada author".into(), ("Ada Author".into(), 5.0));
        taste
            .seed_authors_by_title
            .insert("seed by ada".into(), vec!["Ada Author".into()]);

        let feed = build_discover_feed(&recs, &taste, 8);
        assert!(feed.shelves.iter().any(|s| s.id == "finish_series"));
        assert!(feed
            .shelves
            .iter()
            .any(|s| s.id.starts_with("because:") && !s.items.is_empty()));
    }
}
