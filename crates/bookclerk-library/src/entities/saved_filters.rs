//! `saved_filters` table entity.

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "saved_filters")]
pub struct Model {
    /// Identifier.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Name.
    #[sea_orm(unique)]
    pub name: String,
    /// Query.
    pub query: String,
    /// Created at.
    pub created_at: String,
    /// Updated at.
    pub updated_at: String,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
