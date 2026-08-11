//! `portal_identities` table entity.

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "portal_identities")]
pub struct Model {
    /// Identifier.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Provider.
    pub provider: String,
    /// External user Identifier.
    pub external_user_id: String,
    /// Label.
    pub label: Option<String>,
    /// First-party [`crate::entities::users`] row (Phase 1).
    pub user_id: Option<i64>,
    /// Created at.
    pub created_at: String,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
