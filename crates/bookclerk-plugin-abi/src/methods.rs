//! Workers RPC method names (camelCase).

/// All known method names for discovery / docs.
pub const METHOD_NAMES: &[&str] = &[
    handshake::NAME,
    shutdown::NAME,
    health::NAME,
    diagnose::NAME,
    start::NAME,
    on_event::NAME,
    poll_events::NAME,
    scan_library::NAME,
    sync_listening::NAME,
    authenticate_user::NAME,
    cli_describe::NAME,
    cli_invoke::NAME,
    login::NAME,
    login_start::NAME,
    login_complete::NAME,
    credentials_update::NAME,
    scan::NAME,
    fetch_title::NAME,
    search_catalog::NAME,
    expand_candidates::NAME,
    purchase_hint::NAME,
    list_deals::NAME,
    list_accounts::NAME,
    catalog_detail::NAME,
    put::NAME,
    put_file::NAME,
    get::NAME,
    exists::NAME,
    list::NAME,
    probe::NAME,
    copy::NAME,
    delete::NAME,
    touch_file::NAME,
    db_connect::NAME,
    db_ping::NAME,
    db_query::NAME,
    db_execute::NAME,
];

/// Handshake module.
pub mod handshake {
    /// Name.
    pub const NAME: &str = "handshake";
}
/// Shutdown module.
pub mod shutdown {
    /// Name.
    pub const NAME: &str = "shutdown";
}
/// Health module.
pub mod health {
    /// Name.
    pub const NAME: &str = "health";
}
/// Diagnose module.
pub mod diagnose {
    /// Name.
    pub const NAME: &str = "diagnose";
}
/// Start module.
pub mod start {
    /// Name.
    pub const NAME: &str = "start";
}
/// On event module.
pub mod on_event {
    /// Name.
    pub const NAME: &str = "onEvent";
}
/// Poll events module.
pub mod poll_events {
    /// Name.
    pub const NAME: &str = "pollEvents";
}
/// Scan library module.
pub mod scan_library {
    /// Name.
    pub const NAME: &str = "scanLibrary";
}
/// Sync listening module.
pub mod sync_listening {
    /// Name.
    pub const NAME: &str = "syncListening";
}
/// Authenticate user module.
pub mod authenticate_user {
    /// Name.
    pub const NAME: &str = "authenticateUser";
}
/// CLI describe module.
pub mod cli_describe {
    /// Name.
    pub const NAME: &str = "cliDescribe";
}
/// CLI invoke module.
pub mod cli_invoke {
    /// Name.
    pub const NAME: &str = "cliInvoke";
}
/// Login module.
pub mod login {
    /// Name.
    pub const NAME: &str = "login";
}
/// Login start module.
pub mod login_start {
    /// Name.
    pub const NAME: &str = "loginStart";
}
/// Login complete module.
pub mod login_complete {
    /// Name.
    pub const NAME: &str = "loginComplete";
}
/// Credentials update module.
pub mod credentials_update {
    /// Name.
    pub const NAME: &str = "credentialsUpdate";
}
/// Scan module.
pub mod scan {
    /// Name.
    pub const NAME: &str = "scan";
}
/// Fetch title module.
pub mod fetch_title {
    /// Name.
    pub const NAME: &str = "fetchTitle";
}
/// Search catalog module.
pub mod search_catalog {
    /// Name.
    pub const NAME: &str = "searchCatalog";
}
/// Expand candidates module.
pub mod expand_candidates {
    /// Name.
    pub const NAME: &str = "expandCandidates";
}
/// Purchase hint module.
pub mod purchase_hint {
    /// Name.
    pub const NAME: &str = "purchaseHint";
}
/// List deals module.
pub mod list_deals {
    /// Name.
    pub const NAME: &str = "listDeals";
}
/// List accounts module.
pub mod list_accounts {
    /// Name.
    pub const NAME: &str = "listAccounts";
}
/// Catalog detail module.
pub mod catalog_detail {
    /// Name.
    pub const NAME: &str = "catalogDetail";
}
/// Put module.
pub mod put {
    /// Name.
    pub const NAME: &str = "put";
}
/// Put file module.
pub mod put_file {
    /// Name.
    pub const NAME: &str = "putFile";
}
/// Get module.
pub mod get {
    /// Name.
    pub const NAME: &str = "get";
}
/// Exists module.
pub mod exists {
    /// Name.
    pub const NAME: &str = "exists";
}
/// List module.
pub mod list {
    /// Name.
    pub const NAME: &str = "list";
}
/// Probe module.
pub mod probe {
    /// Name.
    pub const NAME: &str = "probe";
}
/// Copy module.
pub mod copy {
    /// Name.
    pub const NAME: &str = "copy";
}
/// Delete module.
pub mod delete {
    /// Name.
    pub const NAME: &str = "delete";
}
/// Touch file module.
pub mod touch_file {
    /// Name.
    pub const NAME: &str = "touchFile";
}
/// Database connect module.
pub mod db_connect {
    /// Name.
    pub const NAME: &str = "dbConnect";
}
/// Database ping module.
pub mod db_ping {
    /// Name.
    pub const NAME: &str = "dbPing";
}
/// Database query module.
pub mod db_query {
    /// Name.
    pub const NAME: &str = "dbQuery";
}
/// Database execute module.
pub mod db_execute {
    /// Name.
    pub const NAME: &str = "dbExecute";
}

/// Flat constants matching historical `protocol::methods` usage.
pub mod names {
    pub use super::authenticate_user::NAME as AUTHENTICATE_USER;
    pub use super::catalog_detail::NAME as CATALOG_DETAIL;
    pub use super::cli_describe::NAME as CLI_DESCRIBE;
    pub use super::cli_invoke::NAME as CLI_INVOKE;
    pub use super::copy::NAME as COPY;
    pub use super::credentials_update::NAME as CREDENTIALS_UPDATE;
    pub use super::db_connect::NAME as DB_CONNECT;
    pub use super::db_execute::NAME as DB_EXECUTE;
    pub use super::db_ping::NAME as DB_PING;
    pub use super::db_query::NAME as DB_QUERY;
    pub use super::delete::NAME as DELETE;
    pub use super::diagnose::NAME as DIAGNOSE;
    pub use super::exists::NAME as EXISTS;
    pub use super::expand_candidates::NAME as EXPAND_CANDIDATES;
    pub use super::fetch_title::NAME as FETCH_TITLE;
    pub use super::get::NAME as GET;
    pub use super::handshake::NAME as HANDSHAKE;
    pub use super::health::NAME as HEALTH;
    pub use super::list::NAME as LIST;
    pub use super::list_accounts::NAME as LIST_ACCOUNTS;
    pub use super::list_deals::NAME as LIST_DEALS;
    pub use super::login::NAME as LOGIN;
    pub use super::login_complete::NAME as LOGIN_COMPLETE;
    pub use super::login_start::NAME as LOGIN_START;
    pub use super::on_event::NAME as ON_EVENT;
    pub use super::poll_events::NAME as EVENT_POLL;
    pub use super::probe::NAME as PROBE;
    pub use super::purchase_hint::NAME as PURCHASE_HINT;
    pub use super::put::NAME as PUT;
    pub use super::put_file::NAME as PUT_FILE;
    pub use super::scan::NAME as SCAN;
    pub use super::scan_library::NAME as SCAN_LIBRARY;
    pub use super::search_catalog::NAME as SEARCH_CATALOG;
    pub use super::shutdown::NAME as SHUTDOWN;
    pub use super::start::NAME as START;
    pub use super::sync_listening::NAME as SYNC_LISTENING;
    pub use super::touch_file::NAME as TOUCH_FILE;
}
