//! In-process adapters that speak Cap'n Proto `api_version = 2` to external plugin processes.

mod d1_export;
mod database;
mod destination;
mod destination_local;
mod integration;
mod source;

pub use d1_export::{export_d1_sql, export_d1_sql_dump, try_export_d1_sql_dump, D1SnapshotCreds};
pub use database::{
    database_connect_context, load_external_database, migrate_database_plugin,
    migrate_library_schema, open_library_store, open_library_store_for_plugin, DatabaseRegistry,
    ExternalDatabase,
};
pub use destination::{load_external_destinations, DestinationRegistry};
pub use integration::{load_external_integrations, ExternalIntegration};
pub use source::{load_external_sources, ExternalSource};
