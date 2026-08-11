//! `title_request_sources` — per-storefront catalog/pricing snapshot for a wish.

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "title_request_sources")]
pub struct Model {
    /// Identifier.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Title request Identifier.
    pub title_request_id: i64,
    /// Source.
    pub source: String,
    /// Product Identifier.
    pub product_id: String,
    /// Title.
    pub title: Option<String>,
    /// Subtitle.
    pub subtitle: Option<String>,
    /// Authors.
    pub authors: Option<String>,
    /// Narrators.
    pub narrators: Option<String>,
    /// Series.
    pub series: Option<String>,
    /// Series index.
    pub series_index: Option<String>,
    /// Amazon ASIN identifier.
    pub asin: Option<String>,
    /// ISBN identifier.
    pub isbn: Option<String>,
    /// Description.
    pub description: Option<String>,
    /// Publisher.
    pub publisher: Option<String>,
    /// Length minutes.
    pub length_minutes: Option<i64>,
    /// Published at.
    pub published_at: Option<String>,
    /// Categories.
    pub categories: Option<String>,
    /// Language.
    pub language: Option<String>,
    /// Cover URL.
    pub cover_url: Option<String>,
    /// URL.
    pub url: Option<String>,
    /// Price cents.
    pub price_cents: Option<i64>,
    /// Currency.
    pub currency: Option<String>,
    /// Price label.
    pub price_label: Option<String>,
    /// List price cents.
    pub list_price_cents: Option<i64>,
    /// List price label.
    pub list_price_label: Option<String>,
    /// Member price cents.
    pub member_price_cents: Option<i64>,
    /// Member price label.
    pub member_price_label: Option<String>,
    /// Observed at.
    pub observed_at: Option<String>,
    /// Created at.
    pub created_at: String,
    /// Updated at.
    pub updated_at: String,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::title_requests::Entity",
        from = "Column::TitleRequestId",
        to = "super::title_requests::Column::Id",
        on_delete = "Cascade"
    )]
    /// Title request variant.
    TitleRequest,
}

impl Related<super::title_requests::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::TitleRequest.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
