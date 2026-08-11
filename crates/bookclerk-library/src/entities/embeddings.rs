//! `embeddings` table entity.

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "embeddings")]
pub struct Model {
    /// Identifier.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Target kind.
    pub target_kind: String,
    /// Target Identifier.
    pub target_id: String,
    /// Model.
    pub model: String,
    /// Dims.
    pub dims: i64,
    /// Vector.
    pub vector: Vec<u8>,
    /// Text hash.
    pub text_hash: String,
    /// Created at.
    pub created_at: String,
    /// Updated at.
    pub updated_at: String,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
