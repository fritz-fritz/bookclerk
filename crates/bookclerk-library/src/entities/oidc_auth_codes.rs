//! `oidc_auth_codes` — short-lived authorization codes (PKCE).

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "oidc_auth_codes")]
pub struct Model {
    /// Identifier.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Code hash.
    #[sea_orm(unique)]
    pub code_hash: String,
    /// Client Identifier.
    pub client_id: String,
    /// User Identifier.
    pub user_id: i64,
    /// Redirect URI.
    pub redirect_uri: String,
    /// Code challenge.
    pub code_challenge: String,
    /// Code challenge method.
    pub code_challenge_method: String,
    /// Scope.
    pub scope: String,
    /// Expires at.
    pub expires_at: String,
    /// Consumed at.
    pub consumed_at: Option<String>,
    /// Created at.
    pub created_at: String,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
