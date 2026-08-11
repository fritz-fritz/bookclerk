//! `title_requests` table entity.

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "title_requests")]
pub struct Model {
    /// Identifier.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// UUID.
    #[sea_orm(unique)]
    pub uuid: String,
    /// Identity Identifier.
    pub identity_id: Option<i64>,
    /// Title.
    pub title: String,
    /// Authors.
    pub authors: Option<String>,
    /// Amazon ASIN identifier.
    pub asin: Option<String>,
    /// ISBN identifier.
    pub isbn: Option<String>,
    /// Notes.
    pub notes: Option<String>,
    /// Status.
    pub status: String,
    /// Preferred source.
    pub preferred_source: Option<String>,
    /// Work Identifier.
    pub work_id: Option<String>,
    /// Work key.
    pub work_key: String,
    /// Resolved book UUID.
    pub resolved_book_uuid: Option<String>,
    /// Cover URL.
    pub cover_url: Option<String>,
    /// Created at.
    pub created_at: String,
    /// Updated at.
    pub updated_at: String,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
