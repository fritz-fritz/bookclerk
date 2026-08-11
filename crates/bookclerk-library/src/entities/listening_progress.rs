//! `listening_progress` table entity.

use sea_orm::entity::prelude::*;

/// Model.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "listening_progress")]
pub struct Model {
    /// Identifier.
    #[sea_orm(primary_key)]
    pub id: i64,
    /// Identity Identifier.
    pub identity_id: Option<i64>,
    /// Provider.
    pub provider: String,
    /// External user Identifier.
    pub external_user_id: String,
    /// Book UUID.
    pub book_uuid: Option<String>,
    /// Work Identifier.
    pub work_id: Option<String>,
    /// External item Identifier.
    pub external_item_id: String,
    /// Title.
    pub title: Option<String>,
    /// Authors.
    pub authors: Option<String>,
    /// Amazon ASIN identifier.
    pub asin: Option<String>,
    /// ISBN identifier.
    pub isbn: Option<String>,
    /// Progress.
    pub progress: Option<f64>,
    /// Current time seconds.
    pub current_time_seconds: Option<f64>,
    /// Duration seconds.
    pub duration_seconds: Option<f64>,
    /// Is finished.
    pub is_finished: i64,
    /// Last listened at.
    pub last_listened_at: Option<String>,
    /// Updated at.
    pub updated_at: String,
}

/// Relation.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
