//! `oidc_refresh_tokens` — refresh tokens bound to a User.

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "oidc_refresh_tokens")]
pub struct Model {
    /// Identifier.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Token hash.
    #[sea_orm(unique)]
    pub token_hash: String,
    /// Client Identifier.
    pub client_id: String,
    /// User Identifier.
    pub user_id: i64,
    /// Scope.
    pub scope: String,
    /// Expires at.
    pub expires_at: String,
    /// Revoked at.
    pub revoked_at: Option<String>,
    /// Created at.
    pub created_at: String,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
