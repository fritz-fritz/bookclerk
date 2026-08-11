//! `ignored_titles` table entity (composite primary key).

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "ignored_titles")]
pub struct Model {
    /// Source.
    #[sea_orm(primary_key, auto_increment = false)]
    pub source: String,
    /// Account Identifier.
    #[sea_orm(primary_key, auto_increment = false)]
    pub account_id: String,
    /// Product Identifier.
    #[sea_orm(primary_key, auto_increment = false)]
    pub product_id: String,
    /// Reason.
    pub reason: Option<String>,
    /// Created at.
    pub created_at: String,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
