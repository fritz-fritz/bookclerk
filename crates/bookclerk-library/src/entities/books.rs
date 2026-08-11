//! `books` table entity.

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "books")]
pub struct Model {
    /// Identifier.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// UUID.
    #[sea_orm(unique)]
    pub uuid: String,
    /// Source.
    pub source: String,
    /// Account Identifier.
    pub account_id: String,
    /// Product Identifier.
    pub product_id: String,
    /// Amazon ASIN identifier.
    pub asin: Option<String>,
    /// ISBN identifier.
    pub isbn: Option<String>,
    /// Marketplace.
    pub marketplace: String,
    /// Title.
    pub title: String,
    /// Authors.
    pub authors: Option<String>,
    /// Narrators.
    pub narrators: Option<String>,
    /// Series.
    pub series: Option<String>,
    /// Series index.
    pub series_index: Option<String>,
    /// Series Amazon ASIN identifier.
    pub series_asin: Option<String>,
    /// Acquire status.
    pub acquire_status: String,
    /// Storage key.
    pub storage_key: Option<String>,
    /// Error message.
    pub error_message: Option<String>,
    /// Purchased at.
    pub purchased_at: Option<String>,
    /// Tags.
    pub tags: Option<String>,
    /// Rating overall.
    pub rating_overall: Option<f64>,
    /// Rating performance.
    pub rating_performance: Option<f64>,
    /// Rating story.
    pub rating_story: Option<f64>,
    /// Is finished.
    pub is_finished: i64,
    /// Pdf status.
    pub pdf_status: String,
    /// Pdf storage key.
    pub pdf_storage_key: Option<String>,
    /// Publisher.
    pub publisher: Option<String>,
    /// Length minutes.
    pub length_minutes: Option<i64>,
    /// Is abridged.
    pub is_abridged: i64,
    /// Content kind.
    pub content_kind: String,
    /// Categories.
    pub categories: Option<String>,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Published at.
    pub published_at: Option<String>,
    /// Description.
    pub description: Option<String>,
    /// Language.
    pub language: Option<String>,
    /// Cover URL.
    pub cover_url: Option<String>,
    /// Subjects.
    pub subjects: Option<String>,
    /// Enrich source.
    pub enrich_source: Option<String>,
    /// Enrich confidence.
    pub enrich_confidence: Option<f64>,
    /// Enrich updated at.
    pub enrich_updated_at: Option<String>,
    /// Created at.
    pub created_at: String,
    /// Updated at.
    pub updated_at: String,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
