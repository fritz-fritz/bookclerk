//! `security_audit_events` table entity — elevate / impersonate / login audit.

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "security_audit_events")]
pub struct Model {
    /// Identifier.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// At.
    pub at: String,
    /// Actor.
    pub actor: String,
    /// Action.
    pub action: String,
    /// Detail JSON.
    pub detail_json: Option<String>,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
