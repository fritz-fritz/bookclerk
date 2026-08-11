//! `user_invites` table entity — admin/operator invite tickets with role.

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "user_invites")]
pub struct Model {
    /// Identifier.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Token hash.
    #[sea_orm(unique)]
    pub token_hash: String,
    /// Role.
    pub role: String,
    /// Login name.
    pub login_name: Option<String>,
    /// Display name.
    pub display_name: Option<String>,
    /// Expires at.
    pub expires_at: String,
    /// Redeemed at.
    pub redeemed_at: Option<String>,
    /// Created by.
    pub created_by: String,
    /// Created at.
    pub created_at: String,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
