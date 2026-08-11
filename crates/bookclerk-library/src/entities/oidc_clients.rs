//! `oidc_clients` — registered OAuth/OIDC clients (ABS, etc.).

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "oidc_clients")]
pub struct Model {
    /// Identifier.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Client Identifier.
    #[sea_orm(unique)]
    pub client_id: String,
    /// Client secret hash.
    pub client_secret_hash: Option<String>,
    /// Redirect uris JSON.
    pub redirect_uris_json: String,
    /// Name.
    pub name: Option<String>,
    /// Created at.
    pub created_at: String,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
