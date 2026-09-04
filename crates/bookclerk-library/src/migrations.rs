//! Host library schema: frozen plan steps plus the unreleased development pack.
//!
//! Bookclerk has **not** frozen a production v1 schema. [`host_migration_plan`]
//! is empty until a release cut. Live DDL lives in [`UNRELEASED_SQL`]. Fresh
//! databases apply [`current_canonical_schema`] (frozen ups + unreleased) and
//! persist [`crate::SchemaState::Unreleased`] with `base_version` equal to
//! [`SCHEMA_VERSION`] (`0` today: no frozen revisions). Fresh init records each
//! frozen plan step's checksum before the unreleased marker; when the
//! unreleased bucket is empty the database ends [`crate::SchemaState::Frozen`].
//! Adapters lower canonical DDL at
//! the execution edge ([`bookclerk_db_exec::expand_host_schema_batch`]).

use std::sync::OnceLock;

#[cfg(test)]
use std::cell::Cell;

use sha2::{Digest, Sha256};

/// Final SQLite DDL for a fresh Bookclerk library database.
///
/// SeaORM entities in [`crate::entities`] mirror these columns exactly. All
/// integer columns map to `i64`, reals to `f64`, blobs to `Vec<u8>`, and text
/// (including RFC 3339 timestamps) to `String`.
#[must_use]
pub fn latest_schema_sqlite() -> &'static str {
    current_canonical_schema()
}

/// Greenfield SQLite DDL for a fresh library database (unreleased pack).
const SQLITE_SCHEMA: &str = r#"
    CREATE TABLE IF NOT EXISTS accounts (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        account_id TEXT NOT NULL UNIQUE,
        marketplace TEXT NOT NULL,
        label TEXT,
        scan_enabled INTEGER NOT NULL DEFAULT 1,
        source TEXT NOT NULL DEFAULT 'audible',
        connection_status TEXT NOT NULL DEFAULT 'active',
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS books (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        uuid TEXT NOT NULL UNIQUE,
        source TEXT NOT NULL,
        account_id TEXT NOT NULL,
        product_id TEXT NOT NULL,
        asin TEXT,
        isbn TEXT,
        marketplace TEXT NOT NULL,
        title TEXT NOT NULL,
        authors TEXT,
        narrators TEXT,
        series TEXT,
        series_index TEXT,
        series_asin TEXT,
        acquire_status TEXT NOT NULL DEFAULT 'not_acquired',
        storage_key TEXT,
        error_message TEXT,
        purchased_at TEXT,
        tags TEXT,
        rating_overall REAL,
        rating_performance REAL,
        rating_story REAL,
        is_finished INTEGER NOT NULL DEFAULT 0,
        pdf_status TEXT NOT NULL DEFAULT 'not_acquired',
        pdf_storage_key TEXT,
        publisher TEXT,
        length_minutes INTEGER,
        is_abridged INTEGER NOT NULL DEFAULT 0,
        content_kind TEXT NOT NULL DEFAULT 'book',
        categories TEXT,
        subtitle TEXT,
        published_at TEXT,
        description TEXT,
        language TEXT,
        cover_url TEXT,
        subjects TEXT,
        enrich_source TEXT,
        enrich_confidence REAL,
        enrich_updated_at TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        UNIQUE(source, account_id, product_id),
        FOREIGN KEY(account_id) REFERENCES accounts(account_id) ON DELETE CASCADE
    );

    CREATE INDEX IF NOT EXISTS idx_books_uuid ON books(uuid);
    CREATE INDEX IF NOT EXISTS idx_books_status ON books(acquire_status);
    CREATE INDEX IF NOT EXISTS idx_books_account ON books(account_id);
    CREATE INDEX IF NOT EXISTS idx_books_title ON books(title);
    CREATE INDEX IF NOT EXISTS idx_books_pdf_status ON books(pdf_status);
    CREATE INDEX IF NOT EXISTS idx_books_tags ON books(tags);
    CREATE INDEX IF NOT EXISTS idx_books_series_asin ON books(series_asin);
    CREATE INDEX IF NOT EXISTS idx_books_content_kind ON books(content_kind);
    CREATE INDEX IF NOT EXISTS idx_books_isbn ON books(isbn);
    CREATE INDEX IF NOT EXISTS idx_books_source ON books(source);
    CREATE INDEX IF NOT EXISTS idx_books_asin ON books(asin);
    CREATE INDEX IF NOT EXISTS idx_books_product_id ON books(product_id);

    CREATE TABLE IF NOT EXISTS ignored_titles (
        source TEXT NOT NULL,
        account_id TEXT NOT NULL,
        product_id TEXT NOT NULL,
        reason TEXT,
        created_at TEXT NOT NULL,
        PRIMARY KEY (source, account_id, product_id),
        FOREIGN KEY(account_id) REFERENCES accounts(account_id) ON DELETE CASCADE
    );

    CREATE TABLE IF NOT EXISTS saved_filters (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        name TEXT NOT NULL UNIQUE,
        query TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS users (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        role TEXT NOT NULL,
        status TEXT NOT NULL DEFAULT 'active',
        display_name TEXT,
        login_name TEXT,
        email TEXT,
        password_hash TEXT,
        security_version INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        last_seen_at TEXT,
        avatar_source TEXT,
        totp_enabled INTEGER NOT NULL DEFAULT 0
    );

    CREATE INDEX IF NOT EXISTS idx_users_role ON users(role);
    CREATE INDEX IF NOT EXISTS idx_users_status ON users(status);
    CREATE UNIQUE INDEX IF NOT EXISTS idx_users_login_name ON users(login_name);
    CREATE UNIQUE INDEX IF NOT EXISTS idx_users_email ON users(email);

    CREATE TABLE IF NOT EXISTS portal_identities (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        provider TEXT NOT NULL,
        external_user_id TEXT NOT NULL,
        label TEXT,
        user_id INTEGER REFERENCES users(id) ON DELETE SET NULL,
        created_at TEXT NOT NULL,
        picture_url TEXT,
        UNIQUE(provider, external_user_id)
    );
    CREATE INDEX IF NOT EXISTS idx_portal_identities_user ON portal_identities(user_id);

    CREATE TABLE IF NOT EXISTS claim_tickets (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        token_hash TEXT NOT NULL UNIQUE,
        identity_id INTEGER,
        expires_at TEXT NOT NULL,
        redeemed_at TEXT,
        created_by TEXT NOT NULL,
        created_at TEXT NOT NULL,
        FOREIGN KEY(identity_id) REFERENCES portal_identities(id) ON DELETE SET NULL
    );

    CREATE INDEX IF NOT EXISTS idx_claim_tickets_hash ON claim_tickets(token_hash);
    CREATE INDEX IF NOT EXISTS idx_claim_tickets_identity ON claim_tickets(identity_id);

    CREATE TABLE IF NOT EXISTS portal_sessions (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        token_hash TEXT NOT NULL UNIQUE,
        identity_id INTEGER NOT NULL,
        expires_at TEXT NOT NULL,
        created_at TEXT NOT NULL,
        user_agent TEXT,
        device_type TEXT,
        client_label TEXT,
        last_used_at TEXT,
        FOREIGN KEY(identity_id) REFERENCES portal_identities(id) ON DELETE CASCADE
    );

    CREATE INDEX IF NOT EXISTS idx_portal_sessions_hash ON portal_sessions(token_hash);

    CREATE TABLE IF NOT EXISTS operator_sessions (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        token_hash TEXT NOT NULL UNIQUE,
        expires_at TEXT NOT NULL,
        created_at TEXT NOT NULL,
        last_used_at TEXT,
        elevated_from_user_id INTEGER,
        impersonating_user_id INTEGER,
        user_agent TEXT,
        device_type TEXT,
        client_label TEXT
    );

    CREATE INDEX IF NOT EXISTS idx_operator_sessions_hash ON operator_sessions(token_hash);

    CREATE TABLE IF NOT EXISTS security_audit_events (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        at TEXT NOT NULL,
        actor TEXT NOT NULL,
        action TEXT NOT NULL,
        detail_json TEXT
    );

    CREATE INDEX IF NOT EXISTS idx_security_audit_at ON security_audit_events(at);

    CREATE TABLE IF NOT EXISTS account_links (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        identity_id INTEGER NOT NULL,
        account_id TEXT NOT NULL,
        source TEXT NOT NULL,
        created_at TEXT NOT NULL,
        UNIQUE(identity_id, account_id),
        FOREIGN KEY(identity_id) REFERENCES portal_identities(id) ON DELETE CASCADE,
        FOREIGN KEY(account_id) REFERENCES accounts(account_id) ON DELETE CASCADE
    );

    CREATE INDEX IF NOT EXISTS idx_account_links_account ON account_links(account_id);
    CREATE UNIQUE INDEX IF NOT EXISTS idx_account_links_account_exclusive ON account_links(account_id);

    CREATE TABLE IF NOT EXISTS works (
        id TEXT PRIMARY KEY,
        canonical_asin TEXT,
        canonical_isbn TEXT,
        title TEXT NOT NULL,
        authors TEXT,
        narrators TEXT,
        description TEXT,
        subjects TEXT,
        categories TEXT,
        language TEXT,
        series TEXT,
        series_index TEXT,
        cover_url TEXT,
        openlibrary_id TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );

    CREATE INDEX IF NOT EXISTS idx_works_asin ON works(canonical_asin);
    CREATE INDEX IF NOT EXISTS idx_works_isbn ON works(canonical_isbn);
    CREATE INDEX IF NOT EXISTS idx_works_title ON works(title);

    CREATE TABLE IF NOT EXISTS work_editions (
        work_id TEXT NOT NULL,
        book_uuid TEXT NOT NULL UNIQUE,
        created_at TEXT NOT NULL,
        PRIMARY KEY (work_id, book_uuid),
        FOREIGN KEY(work_id) REFERENCES works(id) ON DELETE CASCADE
    );

    CREATE INDEX IF NOT EXISTS idx_work_editions_book ON work_editions(book_uuid);

    CREATE TABLE IF NOT EXISTS listening_progress (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        identity_id INTEGER,
        provider TEXT NOT NULL,
        external_user_id TEXT NOT NULL,
        book_uuid TEXT,
        work_id TEXT,
        external_item_id TEXT NOT NULL,
        title TEXT,
        authors TEXT,
        asin TEXT,
        isbn TEXT,
        progress REAL,
        current_time_seconds REAL,
        duration_seconds REAL,
        is_finished INTEGER NOT NULL DEFAULT 0,
        last_listened_at TEXT,
        updated_at TEXT NOT NULL,
        UNIQUE(provider, external_user_id, external_item_id),
        FOREIGN KEY(identity_id) REFERENCES portal_identities(id) ON DELETE SET NULL
    );

    CREATE INDEX IF NOT EXISTS idx_listening_book ON listening_progress(book_uuid);
    CREATE INDEX IF NOT EXISTS idx_listening_work ON listening_progress(work_id);
    CREATE INDEX IF NOT EXISTS idx_listening_user ON listening_progress(provider, external_user_id);

    CREATE TABLE IF NOT EXISTS title_requests (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        uuid TEXT NOT NULL UNIQUE,
        identity_id INTEGER,
        title TEXT NOT NULL,
        authors TEXT,
        asin TEXT,
        isbn TEXT,
        notes TEXT,
        status TEXT NOT NULL DEFAULT 'open',
        preferred_source TEXT,
        work_id TEXT,
        work_key TEXT NOT NULL DEFAULT '',
        resolved_book_uuid TEXT,
        cover_url TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        FOREIGN KEY(identity_id) REFERENCES portal_identities(id) ON DELETE SET NULL
    );

    CREATE INDEX IF NOT EXISTS idx_title_requests_status ON title_requests(status);
    CREATE INDEX IF NOT EXISTS idx_title_requests_identity ON title_requests(identity_id);
    CREATE INDEX IF NOT EXISTS idx_title_requests_work_key ON title_requests(work_key);
    CREATE INDEX IF NOT EXISTS idx_title_requests_identity_status ON title_requests(identity_id, status);

    CREATE TABLE IF NOT EXISTS title_request_sources (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        title_request_id INTEGER NOT NULL,
        source TEXT NOT NULL,
        product_id TEXT NOT NULL,
        title TEXT,
        subtitle TEXT,
        authors TEXT,
        narrators TEXT,
        series TEXT,
        series_index TEXT,
        asin TEXT,
        isbn TEXT,
        description TEXT,
        publisher TEXT,
        length_minutes INTEGER,
        published_at TEXT,
        categories TEXT,
        language TEXT,
        cover_url TEXT,
        url TEXT,
        price_cents INTEGER,
        currency TEXT,
        price_label TEXT,
        list_price_cents INTEGER,
        list_price_label TEXT,
        member_price_cents INTEGER,
        member_price_label TEXT,
        observed_at TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        UNIQUE(title_request_id, source, product_id),
        FOREIGN KEY(title_request_id) REFERENCES title_requests(id) ON DELETE CASCADE
    );

    CREATE INDEX IF NOT EXISTS idx_trs_request ON title_request_sources(title_request_id);

    CREATE TABLE IF NOT EXISTS embeddings (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        target_kind TEXT NOT NULL,
        target_id TEXT NOT NULL,
        model TEXT NOT NULL,
        dims INTEGER NOT NULL,
        vector BLOB NOT NULL,
        text_hash TEXT NOT NULL,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        UNIQUE(target_kind, target_id, model)
    );

    CREATE INDEX IF NOT EXISTS idx_embeddings_target ON embeddings(target_kind, target_id);

    CREATE TABLE IF NOT EXISTS user_preferences (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        subject_key TEXT NOT NULL UNIQUE,
        identity_id INTEGER,
        default_view TEXT NOT NULL DEFAULT 'discover',
        disabled_shelves_json TEXT NOT NULL DEFAULT '[]',
        discover_sort TEXT NOT NULL DEFAULT 'relevance',
        discover_sort_dir TEXT NOT NULL DEFAULT 'desc',
        discover_language TEXT,
        discover_excluded_sources_json TEXT NOT NULL DEFAULT '[]',
        theme TEXT NOT NULL DEFAULT 'system',
        updated_at TEXT NOT NULL,
        FOREIGN KEY(identity_id) REFERENCES portal_identities(id) ON DELETE CASCADE
    );

    CREATE INDEX IF NOT EXISTS idx_user_preferences_identity ON user_preferences(identity_id);

    CREATE TABLE IF NOT EXISTS encrypted_secrets (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        kind TEXT NOT NULL,
        provider TEXT,
        account_type TEXT NOT NULL DEFAULT 'integration',
        account_id TEXT,
        name TEXT NOT NULL,
        format TEXT NOT NULL DEFAULT 'json',
        ciphertext BLOB NOT NULL,
        kdf_algorithm TEXT,
        kdf_salt BLOB,
        kdf_m_cost INTEGER,
        kdf_t_cost INTEGER,
        kdf_p_cost INTEGER,
        cipher_algorithm TEXT,
        cipher_nonce BLOB,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        UNIQUE(kind, provider, account_type, account_id, name)
    );

    CREATE INDEX IF NOT EXISTS idx_encrypted_secrets_kind ON encrypted_secrets(kind);
    CREATE INDEX IF NOT EXISTS idx_encrypted_secrets_account ON encrypted_secrets(account_id);
    CREATE INDEX IF NOT EXISTS idx_encrypted_secrets_account_type ON encrypted_secrets(account_type);

    CREATE TABLE IF NOT EXISTS user_invites (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        token_hash TEXT NOT NULL UNIQUE,
        role TEXT NOT NULL,
        login_name TEXT,
        display_name TEXT,
        expires_at TEXT NOT NULL,
        redeemed_at TEXT,
        created_by TEXT NOT NULL,
        created_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_user_invites_hash ON user_invites(token_hash);

    CREATE TABLE IF NOT EXISTS oidc_clients (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        client_id TEXT NOT NULL UNIQUE,
        client_secret_hash TEXT,
        redirect_uris_json TEXT NOT NULL,
        name TEXT,
        created_at TEXT NOT NULL,
        issue_refresh_token INTEGER NOT NULL DEFAULT 1,
        allowed_scopes_json TEXT NOT NULL DEFAULT '["openid","profile","email"]',
        enabled INTEGER NOT NULL DEFAULT 1,
        plugin_id TEXT
    );
    CREATE TABLE IF NOT EXISTS oidc_auth_codes (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        code_hash TEXT NOT NULL UNIQUE,
        client_id TEXT NOT NULL,
        user_id INTEGER NOT NULL,
        redirect_uri TEXT NOT NULL,
        code_challenge TEXT NOT NULL,
        code_challenge_method TEXT NOT NULL,
        scope TEXT NOT NULL,
        expires_at TEXT NOT NULL,
        consumed_at TEXT,
        created_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_oidc_auth_codes_hash ON oidc_auth_codes(code_hash);
    CREATE TABLE IF NOT EXISTS oidc_refresh_tokens (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        token_hash TEXT NOT NULL UNIQUE,
        client_id TEXT NOT NULL,
        user_id INTEGER NOT NULL,
        scope TEXT NOT NULL,
        expires_at TEXT NOT NULL,
        revoked_at TEXT,
        created_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_oidc_refresh_hash ON oidc_refresh_tokens(token_hash);

    CREATE TABLE IF NOT EXISTS oidc_rp_states (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        state_hash TEXT NOT NULL UNIQUE,
        provider_id TEXT NOT NULL,
        pkce_verifier TEXT NOT NULL,
        nonce TEXT NOT NULL,
        purpose TEXT NOT NULL,
        user_id INTEGER,
        expires_at TEXT NOT NULL,
        created_at TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS webauthn_credentials (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        user_id INTEGER NOT NULL,
        credential_id TEXT NOT NULL UNIQUE,
        passkey_json TEXT NOT NULL,
        name TEXT,
        created_at TEXT NOT NULL,
        last_used_at TEXT
    );
    CREATE INDEX IF NOT EXISTS idx_webauthn_credentials_user ON webauthn_credentials(user_id);
    CREATE TABLE IF NOT EXISTS webauthn_challenges (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        challenge_id TEXT NOT NULL UNIQUE,
        user_id INTEGER,
        kind TEXT NOT NULL,
        state_json TEXT NOT NULL,
        expires_at TEXT NOT NULL,
        created_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_oidc_rp_states_expires ON oidc_rp_states(expires_at);
    CREATE INDEX IF NOT EXISTS idx_webauthn_challenges_expires ON webauthn_challenges(expires_at);

    CREATE TABLE IF NOT EXISTS db_atomic_receipts (
        operation_id TEXT PRIMARY KEY NOT NULL,
        operation_kind TEXT NOT NULL,
        request_hash TEXT NOT NULL,
        status TEXT NOT NULL,
        payload TEXT,
        created_at TEXT NOT NULL,
        expires_at TEXT NOT NULL,
        consume_key TEXT UNIQUE
    );
    CREATE INDEX IF NOT EXISTS idx_db_atomic_receipts_expires ON db_atomic_receipts(expires_at);

    CREATE TABLE IF NOT EXISTS jobs (
        id TEXT PRIMARY KEY NOT NULL,
        kind TEXT NOT NULL,
        state TEXT NOT NULL,
        priority INTEGER NOT NULL DEFAULT 0,
        resource_class TEXT NOT NULL,
        payload TEXT NOT NULL,
        progress TEXT,
        attempt_count INTEGER NOT NULL DEFAULT 0,
        max_attempts INTEGER NOT NULL DEFAULT 3,
        run_after TEXT NOT NULL,
        lease_owner TEXT,
        lease_expires_at TEXT,
        dedup_key TEXT NOT NULL,
        error_kind TEXT,
        error_message TEXT,
        cancel_requested INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        started_at TEXT,
        finished_at TEXT,
        lease_generation INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX IF NOT EXISTS idx_jobs_claim ON jobs(resource_class, state, run_after, priority);
    CREATE INDEX IF NOT EXISTS idx_jobs_dedup ON jobs(dedup_key, state);
    CREATE INDEX IF NOT EXISTS idx_jobs_state ON jobs(state);
    CREATE UNIQUE INDEX IF NOT EXISTS idx_jobs_dedup_active
        ON jobs(dedup_key) WHERE state IN ('pending', 'running');
    CREATE TABLE IF NOT EXISTS job_temp_paths (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        job_id TEXT NOT NULL,
        path TEXT NOT NULL,
        created_at TEXT NOT NULL,
        reserved_bytes INTEGER NOT NULL DEFAULT 0,
        FOREIGN KEY(job_id) REFERENCES jobs(id) ON DELETE CASCADE
    );
    CREATE INDEX IF NOT EXISTS idx_job_temp_paths_job ON job_temp_paths(job_id);
    CREATE UNIQUE INDEX IF NOT EXISTS idx_job_temp_paths_job_path
        ON job_temp_paths(job_id, path);
    CREATE TABLE IF NOT EXISTS job_queue_control (
        id INTEGER PRIMARY KEY CHECK (id = 1)
    );
    INSERT OR IGNORE INTO job_queue_control (id) VALUES (1);

    CREATE TABLE IF NOT EXISTS domain_events (
        id TEXT PRIMARY KEY NOT NULL,
        event_type TEXT NOT NULL,
        schema_version INTEGER NOT NULL,
        occurred_at TEXT NOT NULL,
        account_id TEXT NOT NULL DEFAULT '',
        source TEXT NOT NULL DEFAULT '',
        correlation_id TEXT NOT NULL DEFAULT '',
        causation_id TEXT NOT NULL DEFAULT '',
        dedup_key TEXT NOT NULL,
        payload TEXT NOT NULL,
        ordering_key TEXT NOT NULL DEFAULT '',
        dispatch_state TEXT NOT NULL,
        created_at TEXT NOT NULL,
        wake_pending INTEGER NOT NULL DEFAULT 1,
        wake_lease_owner TEXT,
        wake_lease_expires_at TEXT,
        wake_cursor_at TEXT NOT NULL DEFAULT '',
        wake_cursor_id TEXT NOT NULL DEFAULT '',
        dispatch_snapshot_json TEXT NOT NULL DEFAULT '',
        UNIQUE(account_id, source, event_type, dedup_key)
    );
    CREATE INDEX IF NOT EXISTS idx_domain_events_dispatch ON domain_events(dispatch_state, created_at);
    CREATE INDEX IF NOT EXISTS idx_domain_events_dispatch_created ON domain_events(dispatch_state, created_at, id);
    CREATE INDEX IF NOT EXISTS idx_domain_events_wake_pending ON domain_events(created_at, id) WHERE wake_pending = 1;
    CREATE TABLE IF NOT EXISTS event_deliveries (
        id TEXT PRIMARY KEY NOT NULL,
        event_id TEXT NOT NULL,
        plugin_id TEXT NOT NULL,
        idempotency_key TEXT NOT NULL UNIQUE,
        state TEXT NOT NULL,
        attempt_count INTEGER NOT NULL DEFAULT 0,
        max_attempts INTEGER NOT NULL DEFAULT 8,
        lease_owner TEXT,
        lease_expires_at TEXT,
        lease_generation INTEGER NOT NULL DEFAULT 0,
        run_after TEXT NOT NULL,
        invocation_sequence INTEGER NOT NULL DEFAULT 0,
        resume_pending INTEGER NOT NULL DEFAULT 0,
        checkpoint_json TEXT,
        checkpoint_schema_version INTEGER NOT NULL DEFAULT 0,
        ordering_key TEXT NOT NULL DEFAULT '',
        outcome TEXT,
        error_message TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        cancel_requested INTEGER NOT NULL DEFAULT 0,
        resource_class TEXT NOT NULL DEFAULT 'network',
        wake_event_type TEXT NOT NULL DEFAULT '',
        wake_filter_json TEXT NOT NULL DEFAULT '',
        wake_grants_json TEXT NOT NULL DEFAULT '',
        UNIQUE(event_id, plugin_id),
        FOREIGN KEY(event_id) REFERENCES domain_events(id) ON DELETE CASCADE
    );
    CREATE INDEX IF NOT EXISTS idx_event_deliveries_claim ON event_deliveries(state, run_after, created_at);
    CREATE INDEX IF NOT EXISTS idx_event_deliveries_plugin_order ON event_deliveries(plugin_id, ordering_key, created_at);
    CREATE INDEX IF NOT EXISTS idx_event_deliveries_state ON event_deliveries(state);
    CREATE INDEX IF NOT EXISTS idx_event_deliveries_wake ON event_deliveries(state, wake_event_type);
    CREATE INDEX IF NOT EXISTS idx_event_deliveries_plugin_running ON event_deliveries(plugin_id, state);

    CREATE TABLE IF NOT EXISTS event_subscriber_nodes (
        node_id TEXT NOT NULL,
        plugin_id TEXT NOT NULL,
        subscriptions_json TEXT NOT NULL,
        enabled INTEGER NOT NULL DEFAULT 1,
        heartbeat_at TEXT NOT NULL,
        PRIMARY KEY (node_id, plugin_id)
    );
    CREATE INDEX IF NOT EXISTS idx_event_subscriber_nodes_heartbeat
        ON event_subscriber_nodes(heartbeat_at);
    CREATE TABLE IF NOT EXISTS event_outbox_stats (
        id INTEGER PRIMARY KEY NOT NULL,
        retries_total INTEGER NOT NULL DEFAULT 0,
        suspensions_total INTEGER NOT NULL DEFAULT 0,
        dead_letters_total INTEGER NOT NULL DEFAULT 0,
        dispatch_latency_ms_sum INTEGER NOT NULL DEFAULT 0,
        dispatch_count INTEGER NOT NULL DEFAULT 0,
        handler_latency_ms_sum INTEGER NOT NULL DEFAULT 0,
        handler_count INTEGER NOT NULL DEFAULT 0
    );
    INSERT OR IGNORE INTO event_outbox_stats (
        id, retries_total, suspensions_total, dead_letters_total,
        dispatch_latency_ms_sum, dispatch_count, handler_latency_ms_sum, handler_count
    ) VALUES (1, 0, 0, 0, 0, 0, 0, 0);

    CREATE TABLE IF NOT EXISTS db_serialization_slots (
        slot_key TEXT PRIMARY KEY NOT NULL,
        bump INTEGER NOT NULL DEFAULT 0
    );

    CREATE TABLE IF NOT EXISTS plugin_databases (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        plugin_id TEXT NOT NULL,
        binding TEXT NOT NULL,
        backend_kind TEXT NOT NULL,
        unit_ref TEXT NOT NULL,
        created_at TEXT NOT NULL,
        UNIQUE(plugin_id, binding)
    );
    CREATE INDEX IF NOT EXISTS idx_plugin_databases_plugin ON plugin_databases(plugin_id);
    "#;

/// Reserved adapter-private catalog + identity tables (canonical SQLite-shaped).
///
/// Applied only inside isolated plugin binding databases, never the host library.
/// Postgres adapters rewrite `last INTEGER` to `BIGINT`. Guests cannot name
/// these tables.
const BINDING_SQL_CATALOG_SQLITE: &str = r#"
    CREATE TABLE IF NOT EXISTS bookclerk_sql_catalog (
        table_name TEXT NOT NULL,
        column_name TEXT NOT NULL,
        sql_type TEXT NOT NULL,
        ordinal INTEGER NOT NULL,
        is_identity INTEGER NOT NULL,
        default_sql TEXT NOT NULL,
        PRIMARY KEY (table_name, column_name)
    );
    CREATE TABLE IF NOT EXISTS bookclerk_sql_schema (
        table_name TEXT PRIMARY KEY NOT NULL,
        fingerprint TEXT NOT NULL,
        identity_column TEXT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS bookclerk_identity (
        table_name TEXT PRIMARY KEY NOT NULL,
        last INTEGER NOT NULL
    );
    CREATE TABLE IF NOT EXISTS bookclerk_sql_ddl (
        kind TEXT NOT NULL,
        name TEXT NOT NULL,
        table_name TEXT NOT NULL,
        canonical_sql TEXT NOT NULL,
        PRIMARY KEY (kind, name)
    );
"#;

/// Highest **frozen** schema version this binary knows (`0` while the plan is empty).
///
/// This is not a discriminator for uninitialized vs unreleased. Use
/// [`crate::SchemaState`].
pub const SCHEMA_VERSION: i64 = 0;

/// Oldest frozen schema version this binary can run. Unused while the plan is empty.
pub const MIN_SUPPORTED_SCHEMA_VERSION: i64 = 1;

/// Live development DDL. Until a release cut this **is** the library schema.
pub const UNRELEASED_SQL: &str = SQLITE_SCHEMA;

/// Host bookkeeping table created before applying plan versions.
pub const SCHEMA_MIGRATIONS_DDL: &str = "CREATE TABLE IF NOT EXISTS schema_migrations (
        version INTEGER NOT NULL,
        state TEXT NOT NULL,
        checksum TEXT NOT NULL,
        app_version TEXT NOT NULL,
        applied_at TEXT NOT NULL,
        PRIMARY KEY (state, version)
    )";

/// One host-owned schema version in the canonical Bookclerk migration plan.
///
/// Marker capabilities ([`crate::HostSchemaKind`]) choose only how each version
/// is recorded (`PRAGMA user_version`, `schema_migrations` row, or one atomic
/// batch). The live connection backend lowers [`Self::canonical`] at the adapter
/// boundary (Postgres) or applies it verbatim (SQLite / D1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostMigrationStep {
    /// `PRAGMA user_version` / `schema_migrations.version` for this step.
    pub version: i64,
    /// Canonical SQLite-shaped Bookclerk DDL for this version (the `up`).
    pub canonical: &'static str,
    /// Reverse DDL when this step is reversible; `None` means restore a backup.
    pub down: Option<&'static str>,
    /// First Bookclerk semver that shipped this step.
    pub introduced_in: &'static str,
}

impl HostMigrationStep {
    /// SHA-256 hex digest of `up` (and `down` when present) used as the freeze lock.
    #[must_use]
    pub fn checksum(&self) -> String {
        migration_step_checksum(self.canonical, self.down)
    }

    /// True when [`Self::down`] is present so CLI rollback can apply this step.
    #[must_use]
    pub fn reversible(&self) -> bool {
        self.down.is_some()
    }
}

/// SHA-256 hex of canonical up SQL, plus down SQL when the step is reversible.
#[must_use]
pub fn migration_step_checksum(canonical: &str, down: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    if let Some(down) = down {
        hasher.update(b"\n-- down\n");
        hasher.update(down.as_bytes());
    }
    hex::encode(hasher.finalize())
}

/// Canonical bootstrap DDL applied inside every isolated plugin binding
/// database at first open.
///
/// Each binding database carries its own `db_atomic_receipts` table so guest
/// retry tokens replay inside the binding, never against the library.
const BINDING_BOOTSTRAP_SQL: &str = r#"
    CREATE TABLE IF NOT EXISTS db_atomic_receipts (
        operation_id TEXT PRIMARY KEY NOT NULL,
        operation_kind TEXT NOT NULL,
        request_hash TEXT NOT NULL,
        status TEXT NOT NULL,
        payload TEXT,
        created_at TEXT NOT NULL,
        expires_at TEXT NOT NULL,
        consume_key TEXT UNIQUE
    );
    CREATE INDEX IF NOT EXISTS idx_db_atomic_receipts_expires ON db_atomic_receipts(expires_at);
"#;

/// Canonical bootstrap DDL applied inside every isolated plugin binding.
#[must_use]
pub fn binding_bootstrap_sql() -> &'static str {
    static SQL: OnceLock<&'static str> = OnceLock::new();
    SQL.get_or_init(|| format!("{BINDING_BOOTSTRAP_SQL}\n{BINDING_SQL_CATALOG_SQLITE}").leak())
}

/// Frozen ups concatenated with [`UNRELEASED_SQL`].
///
/// After a future release cut this is `host_migration_plan` DDL plus whatever
/// is again unreleased — do not assume it equals [`UNRELEASED_SQL`] forever.
#[must_use]
pub fn current_canonical_schema() -> &'static str {
    static SQL: OnceLock<String> = OnceLock::new();
    SQL.get_or_init(|| {
        let mut parts: Vec<&str> = production_host_migration_plan()
            .iter()
            .map(|step| step.canonical)
            .collect();
        if !UNRELEASED_SQL.trim().is_empty() {
            parts.push(UNRELEASED_SQL);
        }
        parts.join("\n")
    })
    .as_str()
}

/// SHA-256 of [`UNRELEASED_SQL`] (empty string when the bucket is empty).
#[must_use]
pub fn unreleased_checksum() -> String {
    migration_step_checksum(UNRELEASED_SQL, None)
}

/// Host table names declared by [`current_canonical_schema`], plus
/// `schema_migrations`.
#[must_use]
pub fn current_canonical_table_names() -> Vec<String> {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES
        .get_or_init(|| {
            let mut names = table_names_from_canonical_ddl(current_canonical_schema());
            if !names.iter().any(|n| n == "schema_migrations") {
                names.push("schema_migrations".into());
            }
            names
        })
        .clone()
}

/// Parses `CREATE TABLE` names from canonical SQLite-shaped DDL.
///
/// Names are lowercased to match mechanical Postgres lowering.
fn table_names_from_canonical_ddl(sql: &str) -> Vec<String> {
    let mut names = Vec::new();
    for stmt in bookclerk_db_exec::split_schema_statements(sql) {
        let trimmed = stmt.trim();
        let lower = trimmed.to_ascii_lowercase();
        let rest = if let Some(rest) = lower.strip_prefix("create table") {
            rest.trim()
        } else {
            continue;
        };
        let rest = rest
            .strip_prefix("if not exists")
            .map(str::trim)
            .unwrap_or(rest);
        let name = rest
            .split(|c: char| c.is_whitespace() || c == '(')
            .find(|part| !part.is_empty())
            .unwrap_or("");
        if !name.is_empty() {
            names.push(name.trim_matches('"').trim_matches('`').to_string());
        }
    }
    names
}

/// Column types implied by [`current_canonical_schema`].
#[must_use]
pub fn host_sql_type_env() -> bookclerk_plugin_abi::SqlTypeEnv {
    bookclerk_plugin_abi::sql_type_env_from_canonical_ddl(current_canonical_schema())
}

/// Frozen host migration steps. Empty until a release cut copies
/// [`UNRELEASED_SQL`] into version 1.
#[must_use]
pub fn host_migration_plan() -> Vec<HostMigrationStep> {
    #[cfg(test)]
    {
        if let Some(plan) = HOST_PLAN_OVERRIDE.with(Cell::get) {
            return plan.to_vec();
        }
    }
    production_host_migration_plan()
}

/// Production frozen plan (empty until a release cut). Test overrides must not
/// feed [`current_canonical_schema`]'s `OnceLock`.
fn production_host_migration_plan() -> Vec<HostMigrationStep> {
    Vec::new()
}

#[cfg(test)]
thread_local! {
    static HOST_PLAN_OVERRIDE: Cell<Option<&'static [HostMigrationStep]>> = const { Cell::new(None) };
}

/// Test-only: [`host_migration_plan`] returns `plan` until the guard drops.
#[cfg(test)]
pub(crate) struct HostPlanOverrideGuard;

#[cfg(test)]
impl Drop for HostPlanOverrideGuard {
    fn drop(&mut self) {
        HOST_PLAN_OVERRIDE.with(|cell| cell.set(None));
    }
}

/// Installs a test-only frozen plan for [`host_migration_plan`] until drop.
#[cfg(test)]
pub(crate) fn override_host_migration_plan(
    plan: &'static [HostMigrationStep],
) -> HostPlanOverrideGuard {
    HOST_PLAN_OVERRIDE.with(|cell| cell.set(Some(plan)));
    HostPlanOverrideGuard
}

/// Final PostgreSQL DDL for a fresh Bookclerk library database.
///
/// The canonical baseline lowered mechanically per statement at the adapter
/// edge — there is no hand-authored parallel Postgres schema.
#[must_use]
pub fn latest_schema_postgres() -> String {
    bookclerk_db_exec::split_schema_statements(current_canonical_schema())
        .into_iter()
        .map(|stmt| bookclerk_db_exec::lower_canonical_ddl_to_postgres(&stmt))
        .collect::<Vec<_>>()
        .join(";\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn apply_current_schema(conn: &Connection) {
        conn.execute_batch(current_canonical_schema()).unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    }

    fn insert_domain_event(conn: &Connection, id: &str, account_id: &str, dedup_key: &str) {
        conn.execute(
            "INSERT INTO domain_events (
                id, event_type, schema_version, occurred_at, account_id, source,
                correlation_id, causation_id, dedup_key, payload, ordering_key,
                dispatch_state, created_at
            ) VALUES (
                ?1, 'book_acquired', 1, '2026-01-01T00:00:00+00:00', ?2,
                'audible', '', '', ?3, '{}', '', 'pending',
                '2026-01-01T00:00:00+00:00'
            )",
            rusqlite::params![id, account_id, dedup_key],
        )
        .unwrap();
    }

    fn insert_delivery(conn: &Connection, id: &str, event_id: &str) -> rusqlite::Result<usize> {
        conn.execute(
            "INSERT INTO event_deliveries (
                id, event_id, plugin_id, idempotency_key, state, run_after,
                created_at, updated_at
            ) VALUES (
                ?1, ?2, 'echo', ?1, 'pending', '2026-01-01T00:00:00+00:00',
                '2026-01-01T00:00:00+00:00', '2026-01-01T00:00:00+00:00'
            )",
            rusqlite::params![id, event_id],
        )
    }

    #[test]
    fn current_schema_rejects_orphan_event_deliveries_with_foreign_keys_on() {
        let conn = Connection::open_in_memory().unwrap();
        apply_current_schema(&conn);
        let orphan = insert_delivery(&conn, "evt-missing:echo", "evt-missing");
        assert!(
            orphan.is_err(),
            "event_deliveries.event_id must reference domain_events(id)"
        );
        insert_domain_event(&conn, "evt-1", "acct", "book_acquired:u1");
        insert_delivery(&conn, "evt-1:echo", "evt-1").unwrap();
        let deliveries: i64 = conn
            .query_row("SELECT COUNT(*) FROM event_deliveries", [], |r| r.get(0))
            .unwrap();
        assert_eq!(deliveries, 1);
    }

    #[test]
    fn current_schema_domain_events_unique_is_namespaced() {
        let conn = Connection::open_in_memory().unwrap();
        apply_current_schema(&conn);
        insert_domain_event(&conn, "evt-1", "acct", "book_acquired:u1");
        insert_domain_event(&conn, "evt-2", "other", "book_acquired:u1");
        let dup = conn.execute(
            "INSERT INTO domain_events (
                id, event_type, schema_version, occurred_at, account_id, source,
                correlation_id, causation_id, dedup_key, payload, ordering_key,
                dispatch_state, created_at
            ) VALUES (
                'evt-3', 'book_acquired', 1, '2026-01-01T00:00:00+00:00', 'acct',
                'audible', '', '', 'book_acquired:u1', '{}', '', 'pending',
                '2026-01-01T00:00:00+00:00'
            )",
            [],
        );
        assert!(
            dup.is_err(),
            "UNIQUE(account_id, source, event_type, dedup_key) must hold"
        );
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM domain_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn host_migration_plan_is_empty_until_a_release_cut() {
        assert!(host_migration_plan().is_empty());
        assert_eq!(SCHEMA_VERSION, 0);
        assert!(!UNRELEASED_SQL.trim().is_empty());
        assert!(UNRELEASED_SQL.contains("plugin_databases"));
        assert!(UNRELEASED_SQL.contains("dispatch_snapshot_json"));
        assert!(UNRELEASED_SQL.contains("db_serialization_slots"));
        assert_eq!(current_canonical_schema(), UNRELEASED_SQL);
        assert_eq!(unreleased_checksum().len(), 64);
        let tables = current_canonical_table_names();
        assert!(tables.contains(&"books".into()));
        assert!(tables.contains(&"schema_migrations".into()));
        assert!(tables.contains(&"plugin_databases".into()));
    }

    #[test]
    fn unreleased_checksum_is_stable() {
        let a = unreleased_checksum();
        let b = unreleased_checksum();
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn postgres_lowering_of_baseline_is_mechanically_complete() {
        let lowered = latest_schema_postgres();
        // No SQLite-isms may survive the mechanical lowering; the CI Postgres
        // sidecar applies this exact output (`postgres_test_store`).
        for token in [
            "AUTOINCREMENT",
            " INTEGER",
            " BLOB",
            " REAL",
            "INSERT OR IGNORE",
        ] {
            assert!(
                !lowered.contains(token),
                "sqlite-ism `{token}` survived postgres lowering"
            );
        }
        assert!(lowered.contains("BIGINT PRIMARY KEY"), "identity ids");
        assert!(
            lowered.contains("ON CONFLICT DO NOTHING"),
            "insert-or-ignore"
        );
        assert!(lowered.contains(" BYTEA"), "blob columns");
        assert!(lowered.contains(" DOUBLE PRECISION"), "real columns");
        assert!(lowered.contains("UNIQUE(account_id, source, event_type, dedup_key)"));
        // Word-boundary safety: string literals stay untouched.
        assert!(lowered.contains(r#"'["openid","profile","email"]'"#));
        assert!(!lowered.contains("domain_events_v27"));
    }

    #[test]
    fn host_sql_type_env_seeds_library_tables() {
        let env = host_sql_type_env();
        assert!(
            env.has_table("accounts"),
            "expected accounts in {:?}",
            env.iter().map(|(t, _, _)| t).collect::<Vec<_>>()
        );
        assert!(
            env.column_type("portal_identities", "user_id").is_some(),
            "ALTER ADD COLUMN user_id must land in the host type env"
        );
        assert!(
            env.has_table("domain_events"),
            "rebuild RENAME must restore domain_events"
        );
        assert!(
            !env.has_table("domain_events_v27"),
            "v27 rebuild table must be renamed away"
        );
        let req = bookclerk_plugin_abi::ExecuteRequest {
            operation_id: "host-order".into(),
            request_hash: String::new(),
            deadline_unix_ms: 0,
            statements: vec![bookclerk_plugin_abi::TypedDbStatement {
                sql: "SELECT title FROM books ORDER BY title".into(),
                parameters: Vec::new(),
                kind: bookclerk_plugin_abi::DbPlanStatementKind::Select,
                max_rows: 8,
                result_selection: bookclerk_plugin_abi::DbResultSelection::Rows,
            }],
        };
        let proofs = bookclerk_plugin_abi::typecheck_execute_request_proofs(&req, &env)
            .expect("host ORDER BY TEXT must typecheck against the canonical host schema");
        assert!(
            !proofs[0].text_collate_sites.is_empty(),
            "host TEXT ORDER BY must record collate sites: {:?}",
            proofs[0]
        );
        let mut working = env.clone();
        for stmt in bookclerk_db_exec::split_schema_statements(latest_schema_sqlite()) {
            bookclerk_plugin_abi::apply_schema_sql_to_env(&mut working, &stmt);
            if bookclerk_plugin_abi::statement_is_ddl(&stmt) {
                continue;
            }
            let upper = stmt.trim().to_ascii_uppercase();
            if upper.starts_with("PRAGMA ") || upper.starts_with("ALTER ") {
                continue;
            }
            let one = bookclerk_plugin_abi::ExecuteRequest {
                operation_id: "host-ddl-dml".into(),
                request_hash: String::new(),
                deadline_unix_ms: 0,
                statements: vec![bookclerk_plugin_abi::TypedDbStatement {
                    sql: stmt.clone(),
                    parameters: Vec::new(),
                    kind: bookclerk_plugin_abi::DbPlanStatementKind::Execute,
                    max_rows: 0,
                    result_selection: bookclerk_plugin_abi::DbResultSelection::Discard,
                }],
            };
            bookclerk_plugin_abi::typecheck_execute_request_proofs(&one, &working)
                .unwrap_or_else(|err| panic!("host schema DML failed on `{stmt}`: {err}"));
        }
    }
}
