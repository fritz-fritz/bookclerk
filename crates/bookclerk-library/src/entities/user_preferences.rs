//! `user_preferences` table entity.

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "user_preferences")]
pub struct Model {
    /// Identifier.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Subject key.
    #[sea_orm(unique)]
    pub subject_key: String,
    /// Identity Identifier.
    pub identity_id: Option<i64>,
    /// Default view.
    pub default_view: String,
    /// Disabled shelves JSON.
    pub disabled_shelves_json: String,
    /// Discover sort.
    pub discover_sort: String,
    /// Discover sort dir.
    pub discover_sort_dir: String,
    /// Discover language.
    pub discover_language: Option<String>,
    /// Discover excluded sources JSON.
    pub discover_excluded_sources_json: String,
    /// Updated at.
    pub updated_at: String,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
