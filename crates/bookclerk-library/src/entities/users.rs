//! `users` table entity — first-party identity principal (#117).

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    /// Identifier.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// `administrator` or `member`.
    pub role: String,
    /// `active` or `disabled`.
    pub status: String,
    /// Display name.
    pub display_name: Option<String>,
    /// Local login name (email/username); unique when set.
    pub login_name: Option<String>,
    /// Argon2id hash; null until local password is set (Phase 3).
    pub password_hash: Option<String>,
    /// Security version.
    pub security_version: i64,
    /// Created at.
    pub created_at: String,
    /// Updated at.
    pub updated_at: String,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
