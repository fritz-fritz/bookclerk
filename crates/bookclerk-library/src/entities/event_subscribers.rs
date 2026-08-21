//! SeaORM entity for the cluster-authoritative `event_subscribers` catalog.

use sea_orm::entity::prelude::*;

/// Row shape for this table (`DeriveEntityModel`).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "event_subscribers")]
pub struct Model {
    /// Subscriber plugin id (primary key; last writer per id wins).
    #[sea_orm(primary_key, auto_increment = false)]
    pub plugin_id: String,
    /// JSON array of catalog subscriptions (`type`, `schema_versions`, …).
    pub subscriptions_json: String,
    /// `1` when this plugin should receive matching deliveries.
    pub enabled: i64,
    /// RFC 3339 last upsert time.
    pub updated_at: String,
}

/// Declared SeaORM relations (catalog rows are looked up by `plugin_id`).
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
