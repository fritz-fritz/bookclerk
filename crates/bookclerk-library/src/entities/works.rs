//! `works` table entity (text primary key).

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "works")]
pub struct Model {
    /// Identifier.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// Canonical Amazon ASIN identifier.
    pub canonical_asin: Option<String>,
    /// Canonical ISBN identifier.
    pub canonical_isbn: Option<String>,
    /// Title.
    pub title: String,
    /// Authors.
    pub authors: Option<String>,
    /// Narrators.
    pub narrators: Option<String>,
    /// Description.
    pub description: Option<String>,
    /// Subjects.
    pub subjects: Option<String>,
    /// Categories.
    pub categories: Option<String>,
    /// Language.
    pub language: Option<String>,
    /// Series.
    pub series: Option<String>,
    /// Series index.
    pub series_index: Option<String>,
    /// Cover URL.
    pub cover_url: Option<String>,
    /// Openlibrary Identifier.
    pub openlibrary_id: Option<String>,
    /// Created at.
    pub created_at: String,
    /// Updated at.
    pub updated_at: String,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
