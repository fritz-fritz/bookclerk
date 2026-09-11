//! External plugin host for Bookclerk (`bookclerk-plugin-host`).
//!
//! Discovers staged guests under `$BOOKCLERK_FILES_DIR/plugins/` and spawns
//! them over the Workers RPC ABI through the mandatory `bookclerk-workerd`
//! front door (`api_version = 3` Cap'n Proto on stdio).
//!
//! Production hosts never link ordinary plugin implementation crates.
//! First-party storefronts (Audible, Libro.fm, …) are staged guests, the
//! same path third-party plugins use. Database adapter crates
//! (`bookclerk-plugin-database-*`) remain linked for **host-owned** SQL
//! lowering, not in-process plugin execution.
//!
//! External plugins are **untrusted** relative to the host: the host never
//! passes `library.db` / `master.key` / the files-dir root, clears
//! secret-bearing env on spawn, and mediates credentials + library upserts.
//! Operators must `bookclerk plugins approve` domains/bindings before enable;
//! the same covering grant is required again at every external spawn and at
//! privileged delivery points (`config` / `secrets` / `work_fs` / `oauth`).
//! Durable identity is provenance-qualified [`bookclerk_plugin_catalog::PluginKey`];
//! the manifest `id` is a display / CLI alias.
//!
//! Host binaries should depend on **this** crate for registration — not on
//! individual store crates.
//!
//! # Guest SDK
//!
//! Third-party Rust plugins should depend on [`bookclerk_plugin_sdk`], not this
//! crate. This host crate re-exports the protocol types for in-tree convenience.
//!
//! See `docs/plugins.md`, `docs/adr/plugin-workers-rpc-workerd.md`, and
//! `docs/plugin-registry.md`.

mod authority;
mod builtins;
mod callback_proxy;
mod consent;
mod crates_io;
mod destinations;
mod discover;
mod error;
mod event_publisher;
mod host;
mod jail;
mod manifest;
mod registry;
mod rpc;
mod rpc_session;
mod spawn_plan;
mod spawn_stdio;

pub use bookclerk_plugin_sdk::protocol;
pub use bookclerk_plugin_sdk::{
    methods, CatalogHit, CliArg, CliArgKind, CliArgSpec, CliCommandSpec, CliInvokeParams,
    CliInvokeResult, CliSchema, EventPollResult, ExpandCandidatesParams, ExternalUser,
    FetchTitleParams, ListeningProgress, LoginCompleteParams, LoginParams, LoginResult,
    LoginStartResult, PlainFetch, PlainPart, PluginDescribe, PurchaseHint, PurchaseHintParams,
    ScanBook, ScanParams, ScanSummary, SearchCatalogParams, SourceAccount, SyncListeningResult,
    PRODUCT_API_VERSION, PROTOCOL_NAME,
};

pub use authority::{
    authority_revision, fence_plugin_key, fence_stale_sessions, is_fenced, register_session,
    unregister_session,
};
pub use bookclerk_plugin_manifest::TcpGrant;
pub use bookclerk_plugin_sdk::{JobCheckpoint, JobInvocationLease, JobOutcome};
pub use builtins::{load_integrations, load_sources};
pub use consent::{
    active_processes_for, consent_request, consent_request_alias, consent_summary,
    cores_to_percent, database_binding_name, effective_cpu_cores, effective_cpu_rate_percent,
    effective_disk_budget_bytes, effective_disk_mib, effective_extra_processes, effective_grant,
    effective_memory_mib, format_cpu_cores, grant_covers, grant_has_binding, grant_revision,
    grant_within_ceiling, granted_database_bindings, host_cpu_cores_max, host_cpu_rate_max,
    host_logical_cpus, inject_workerd_grant_env, jail_process_overhead, network_compatible,
    overlay_host_implied_network, percent_to_cores, require_binding, require_grant,
    spawn_config_for_grant, spawn_grant, validate_approved_grant, validate_described_capabilities,
    PluginGrant, PluginGrantStore, GRANTS_FILE, KNOWN_HOST_BINDINGS,
    PLUGIN_JAIL_ACTIVE_PROCESSES_MAX, PLUGIN_JAIL_CPU_CORES_DEFAULT, PLUGIN_JAIL_CPU_RATE_DEFAULT,
    PLUGIN_JAIL_CPU_RATE_MAX, PLUGIN_JAIL_EXTRA_PROCESSES_DEFAULT, PLUGIN_JAIL_EXTRA_PROCESSES_MAX,
    PLUGIN_JAIL_MEMORY_MIB_DEFAULT, PLUGIN_JAIL_MEMORY_MIB_MAX, PLUGIN_STATE_BUDGET_MIB_DEFAULT,
    PLUGIN_STATE_BUDGET_MIB_MAX, WORKERD_GRANT_CPU_MS_ENV, WORKERD_GRANT_DOMAINS_ENV,
    WORKERD_GRANT_NETWORK_MODE_ENV, WORKERD_GRANT_POLICY_ENV, WORKERD_GRANT_SUBREQUESTS_ENV,
};
pub use crates_io::search_crates_io;
pub use destinations::{build_acquire_destinations, build_storage_backend};
pub use discover::{
    discover_plugins, plugin_search_dirs, resolve_plugin_ref, settings_table, settings_table_for,
    DiscoveredPlugin,
};
pub use error::{PluginError, Result};
pub use event_publisher::{EventOutbox, OutboxEventPublisher};
pub use host::{
    backup_adapter_id, database_connect_bindings, export_registered_plugin_units,
    load_external_database, load_external_destinations, load_external_integrations,
    load_external_sources, migrate_database_plugin, migrate_library_schema, open_library_store,
    open_library_store_for_plugin, restore_plugin_backup_units, DatabaseRegistry,
    DestinationRegistry, ExternalDatabase, ExternalIntegration, ExternalSource,
};
pub use jail::plugin_data_dir;
pub use manifest::{
    embedded_logo_api_path, entrypoint_family, logo_content_type, validate_logo,
    BindingCapabilities, CapabilitiesManifest, DatabaseBindingManifest, Entrypoint, EventConsumer,
    EventProducer, EventsManifest, JailNetworkNeed, LogoKind, ModuleSpec, NamedBinding,
    NetworkCapabilities, NetworkMode, PluginFamily, PluginManifest, PluginRuntimeKind,
    TriggersManifest, WorkerdLimits, WorkerdRuntimeManifest, ALL_ENTRYPOINTS,
    MAX_EMBEDDED_LOGO_BYTES,
};
pub use registry::{
    family_keyword, host_target_triple, validate_plugin_id, BookclerkPackageMetadata,
    PluginCatalogEntry, PluginCrateName, CRATE_NAME_PREFIX, PRODUCT_KEYWORD, REGISTRY_KEYWORD,
};
pub use rpc_session::{
    plugin_instance_key, ExecutorIdentity, GuestDatabaseFactory, PluginSession, PluginStorage,
    RpcBackupOps, SessionServices, HOST_SHARED_ACCOUNT, OPERATOR_ACCOUNT,
};
pub use spawn_plan::{
    GuestRuntimeKind, SpawnPlan, SpawnTransport, WorkerdFrontDoor, NATIVE_BACKEND_ENV,
    NESTED_JAIL_BIN_ENV, NESTED_NATIVE_JAIL_ENV, WORKERD_BIN_ENV, WORKERD_LAUNCHER_ENV,
};

/// Register discovered external plugins into the in-process registries.
///
/// # Errors
///
/// Returns an error when the operation fails.
pub async fn register_discovered(
    config: &bookclerk_config::Config,
    sources: &mut bookclerk_source::SourceRegistry,
    integrations: &mut bookclerk_integrations::IntegrationRegistry,
) -> Result<()> {
    let plugins = discover_plugins(config)?;
    for plugin in plugins {
        for family in plugin.manifest.families() {
            match family {
                PluginFamily::Source => {
                    if !config.sources.is_enabled(&plugin.manifest.id) {
                        tracing::debug!(
                            id = %plugin.manifest.id,
                            "external source plugin disabled in config; skipping"
                        );
                        continue;
                    }
                    if sources.get(plugin.plugin_key().canonical()).is_some() {
                        tracing::debug!(
                            plugin_key = %plugin.plugin_key().canonical(),
                            alias = %plugin.manifest.id,
                            path = %plugin.root.join("plugin.toml").display(),
                            "skipping external source — PluginKey already registered"
                        );
                        continue;
                    }
                    match ExternalSource::spawn(&plugin, config).await {
                        Ok(source) => {
                            tracing::info!(
                                id = %plugin.manifest.id,
                                path = %plugin.command.display(),
                                "registered external source plugin"
                            );
                            sources.register(std::sync::Arc::new(source));
                        }
                        Err(err) => {
                            tracing::warn!(
                                id = %plugin.manifest.id,
                                %err,
                                "failed to start external source plugin; skipping"
                            );
                        }
                    }
                }
                PluginFamily::Integration => {
                    if !config.integrations.is_enabled(&plugin.manifest.id) {
                        tracing::debug!(
                            id = %plugin.manifest.id,
                            "external integration plugin disabled in config; skipping"
                        );
                        continue;
                    }
                    if integrations.get(plugin.plugin_key().canonical()).is_some() {
                        tracing::debug!(
                            plugin_key = %plugin.plugin_key().canonical(),
                            alias = %plugin.manifest.id,
                            path = %plugin.root.join("plugin.toml").display(),
                            "skipping external integration — PluginKey already registered"
                        );
                        continue;
                    }
                    match ExternalIntegration::spawn(&plugin, config).await {
                        Ok(integration) => {
                            tracing::info!(
                                id = %plugin.manifest.id,
                                path = %plugin.command.display(),
                                "registered external integration plugin"
                            );
                            integrations.register(std::sync::Arc::new(integration));
                        }
                        Err(err) => {
                            tracing::warn!(
                                id = %plugin.manifest.id,
                                %err,
                                "failed to start external integration plugin; skipping"
                            );
                        }
                    }
                }
                PluginFamily::Output => {
                    if !config.output.s3.enabled || plugin.manifest.id != "s3" {
                        tracing::debug!(
                            id = %plugin.manifest.id,
                            "external output plugin skipped (enable [output.s3] for id=s3)"
                        );
                        continue;
                    }
                    tracing::info!(
                        id = %plugin.manifest.id,
                        "discovered output plugin (loaded via load_external_destinations at startup)"
                    );
                }
                PluginFamily::Database => {
                    if plugin
                        .manifest
                        .id
                        .eq_ignore_ascii_case(&config.database.plugin)
                    {
                        tracing::info!(
                            id = %plugin.manifest.id,
                            "discovered database plugin (loaded via load_external_database at startup)"
                        );
                    } else {
                        tracing::debug!(
                            id = %plugin.manifest.id,
                            active = %config.database.plugin,
                            "external database plugin skipped (not [database].plugin)"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
