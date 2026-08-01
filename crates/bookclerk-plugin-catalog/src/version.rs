//! Lightweight semver parsing for registry version selection.

use std::cmp::Ordering;

/// Parsed `major.minor.patch` with optional pre-release ignored for ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// Parse a version string (`1.2.3`, `1.2.3-beta`, `v1.2.3`).
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('v');
        let core = s.split(['-', '+']).next().unwrap_or(s);
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().unwrap_or("0").parse().ok()?;
        let patch = parts.next().unwrap_or("0").parse().ok()?;
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
    }
}

/// Return the greatest semver among `versions`, falling back to lexical max
/// when nothing parses.
#[must_use]
pub fn max_version<'a>(versions: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let mut best: Option<(Version, &'a str)> = None;
    let mut lexical: Option<&'a str> = None;
    for v in versions {
        lexical = match lexical {
            Some(cur) if cur >= v => Some(cur),
            _ => Some(v),
        };
        if let Some(parsed) = Version::parse(v) {
            best = match best {
                Some((cur, s)) if cur >= parsed => Some((cur, s)),
                _ => Some((parsed, v)),
            };
        }
    }
    best.map(|(_, s)| s).or(lexical)
}

/// Newest version strictly greater than `current`, if any.
#[must_use]
pub fn newest_newer_than<'a>(
    current: &str,
    versions: impl IntoIterator<Item = &'a str>,
) -> Option<&'a str> {
    let current = Version::parse(current)?;
    let mut best: Option<(Version, &'a str)> = None;
    for v in versions {
        let Some(parsed) = Version::parse(v) else {
            continue;
        };
        if parsed <= current {
            continue;
        }
        best = match best {
            Some((cur, s)) if cur >= parsed => Some((cur, s)),
            _ => Some((parsed, v)),
        };
    }
    best.map(|(_, s)| s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_orders_not_lexically() {
        assert!(Version::parse("10.0.0").unwrap() > Version::parse("2.0.0").unwrap());
        assert_eq!(max_version(["2.0.0", "10.0.0", "3.1.0"]), Some("10.0.0"));
    }

    #[test]
    fn newest_newer_than_skips_equal() {
        assert_eq!(
            newest_newer_than("1.2.0", ["1.0.0", "1.2.0", "1.3.0", "2.0.0"]),
            Some("2.0.0")
        );
        assert_eq!(newest_newer_than("2.0.0", ["1.0.0", "2.0.0"]), None);
    }
}
