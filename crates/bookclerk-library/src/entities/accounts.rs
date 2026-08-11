//! `accounts` table entity.

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "accounts")]
pub struct Model {
    /// Identifier.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Account Identifier.
    #[sea_orm(unique)]
    pub account_id: String,
    /// Marketplace.
    pub marketplace: String,
    /// Label.
    pub label: Option<String>,
    /// Scan enabled.
    pub scan_enabled: i64,
    /// Source.
    pub source: String,
    /// Connection status.
    pub connection_status: String,
    /// Created at.
    pub created_at: String,
    /// Updated at.
    pub updated_at: String,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
