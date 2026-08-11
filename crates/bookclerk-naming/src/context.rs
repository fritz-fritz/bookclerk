//! Public evaluation context types.

use chrono::NaiveDateTime;

/// An author or narrator (parsed lazily into name components).
#[derive(Debug, Clone, Default)]
pub struct Contributor {
    /// Name.
    pub name: String,
    /// Identifier.
    pub id: Option<String>,
}

impl Contributor {
    /// New.
    pub fn new(name: impl Into<String>, id: Option<String>) -> Self {
        Self {
            name: name.into(),
            id,
        }
    }
}

/// A series membership.
#[derive(Debug, Clone, Default)]
pub struct Series {
    /// Name.
    pub name: String,
    /// Raw order string (e.g. `"1"`, `"1-5"`, `"6"`).
    pub order: Option<String>,
    /// Identifier.
    pub id: Option<String>,
}

impl Series {
    /// New.
    pub fn new(name: impl Into<String>, order: Option<String>, id: Option<String>) -> Self {
        Self {
            name: name.into(),
            order,
            id,
        }
    }
}

/// The kind of content a book represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentKind {
    /// Book variant.
    #[default]
    Book,
    /// Episode variant.
    Episode,
    /// Podcast variant.
    Podcast,
    /// Podcast parent variant.
    PodcastParent,
}

/// All data required to evaluate a naming template for a book.
#[derive(Debug, Clone, Default)]
pub struct BookContext {
    /// Canonical title id (ISBN / ASIN fallback) — used by `<id>`.
    pub isbn: String,
    /// Audible title (no subtitle) — used by `<audible title>` and `<title short>`.
    pub title: Option<String>,
    /// Audible subtitle — used by `<audible subtitle>`.
    pub subtitle: Option<String>,
    /// Full title with subtitle — used by `<title>`. Falls back to [`Self::title`].
    pub title_with_subtitle: Option<String>,

    /// Authors.
    pub authors: Vec<Contributor>,
    /// Narrators.
    pub narrators: Vec<Contributor>,
    /// Series.
    pub series: Vec<Series>,
    /// Tags.
    pub tags: Vec<String>,

    /// Account.
    pub account: Option<String>,
    /// Account nickname.
    pub account_nickname: Option<String>,
    /// Locale.
    pub locale: Option<String>,
    /// Language.
    pub language: Option<String>,

    /// Year published.
    pub year_published: Option<i32>,
    /// Publication date — used by `<pub date>`.
    pub published_at: Option<NaiveDateTime>,
    /// Date added to the account — used by `<date added>`.
    pub purchased_at: Option<NaiveDateTime>,
    /// File date — used by `<file date>`.
    pub file_date: Option<NaiveDateTime>,

    /// Length minutes.
    pub length_minutes: Option<f64>,
    /// Bitrate.
    pub bitrate: Option<i64>,
    /// Samplerate.
    pub samplerate: Option<i64>,
    /// Channels.
    pub channels: Option<i64>,
    /// Codec.
    pub codec: Option<String>,

    /// Is abridged.
    pub is_abridged: bool,
    /// Content kind.
    pub content_kind: ContentKind,

    /// Publisher.
    pub publisher: Option<String>,
    /// Categories.
    pub categories: Vec<String>,

    /// Bookclerk version.
    pub bookclerk_version: Option<String>,
    /// File version.
    pub file_version: Option<String>,
}

impl BookContext {
    /// `<title>` value (full title with subtitle, or audible title).
    pub(crate) fn full_title(&self) -> Option<String> {
        self.title_with_subtitle
            .clone()
            .or_else(|| self.title.clone())
    }

    /// `<title short>`: audible title truncated at the first colon.
    pub(crate) fn title_short(&self) -> Option<String> {
        let title = self.title.as_deref()?;
        Some(match title.find(':') {
            Some(i) => title[..i].to_string(),
            None => title.to_string(),
        })
    }

    pub(crate) fn is_series(&self) -> bool {
        !self.series.is_empty()
    }

    pub(crate) fn is_podcast(&self) -> bool {
        matches!(
            self.content_kind,
            ContentKind::Podcast | ContentKind::PodcastParent | ContentKind::Episode
        )
    }

    pub(crate) fn is_podcast_parent(&self) -> bool {
        matches!(
            self.content_kind,
            ContentKind::PodcastParent | ContentKind::Podcast
        )
    }
}

/// Per-chapter context for chapter file / title templates.
#[derive(Debug, Clone, Default)]
pub struct ChapterContext {
    /// 1-based chapter position (`<ch#>`).
    pub chapter_number: u32,
    /// Total number of chapters (`<ch count>`).
    pub chapter_count: u32,
    /// Chapter title (`<ch title>`).
    pub chapter_title: Option<String>,
    /// Optional chapter file date override (`<file date>`).
    pub file_date: Option<NaiveDateTime>,
}
