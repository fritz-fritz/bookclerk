//! `operator_sessions` table entity.

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "operator_sessions")]
pub struct Model {
    /// Identifier.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Token hash.
    #[sea_orm(unique)]
    pub token_hash: String,
    /// Expires at.
    pub expires_at: String,
    /// Created at.
    pub created_at: String,
    /// Last used at.
    pub last_used_at: Option<String>,
    /// Set when an Administrator elevates to Operator (Phase 2).
    pub elevated_from_user_id: Option<i64>,
    /// Operator impersonation target user id (Phase 2).
    pub impersonating_user_id: Option<i64>,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
