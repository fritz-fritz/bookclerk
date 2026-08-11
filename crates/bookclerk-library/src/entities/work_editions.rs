//! `work_editions` table entity (composite primary key).

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "work_editions")]
pub struct Model {
    /// Work Identifier.
    #[sea_orm(primary_key, auto_increment = false)]
    pub work_id: String,
    /// Book UUID.
    #[sea_orm(primary_key, auto_increment = false)]
    pub book_uuid: String,
    /// Created at.
    pub created_at: String,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
