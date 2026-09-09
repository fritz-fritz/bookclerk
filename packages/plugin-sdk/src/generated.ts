/**
 * GENERATED FILE - do not edit. Run `python3 scripts/gen-plugin-abi.py --write` after changing crates/bookclerk-plugin-abi/schema/plugin.capnp.
 *
 * Author-facing TypeScript projection of every struct, union, and
 * interface in `crates/bookclerk-plugin-abi/schema/plugin.capnp`. Field
 * names are the schema names (camelCase). Cap'n Proto unions become
 * discriminated `{ kind, value }` unions; `Void` members carry no
 * `value`. `Int64` is `bigint` (exact SQL integers); `UInt64` counters,
 * sizes, and unix-millisecond timestamps are `number`. `Data` is
 * `Uint8Array`. Interfaces are capabilities: their methods return
 * promises and the wire codecs carry them through a transport cap table.
 *
 * @module
 */

import type {
  PluginErrorCode,
  CliArgKind,
  DbType,
  DbStatementKind,
  DbResultSelection,
  IsolationReq,
  ResolvedSqlType,
  IntegerArithKind,
} from "./abi.js";

export type {
  PluginErrorCode,
  CliArgKind,
  DbType,
  DbStatementKind,
  DbResultSelection,
  IsolationReq,
  ResolvedSqlType,
  IntegerArithKind,
};

/** Arbitrary JSON value carried inside a `$jsonValue` `Text` field. */
export type JsonValue = unknown;

/** JSON object carried inside a `$jsonValue` `Text` field. */
export type JsonObject = Record<string, unknown>;

export interface ScalarLimits {
  maxScalarBytes: number;
  maxStreamWindowBytes: number;
  maxListPage: number;
}

/**
 * `code` is a snake_case string (`not_found`, `invalid_cursor`, …). Unknown
 * codes are forwarded as-is; SDKs surface them as PluginErrorCode.unknown
 * while retaining the raw wire code.
 */
export interface PluginError {
  code: string;
  message: string;
}

export interface ObjectMetadata {
  key: string;
  size: number;
  contentType: string;
  etag: string;
  sha256: Uint8Array;
}

export interface ObjectInfo {
  key: string;
  size: number;
}

export interface ListOptions {
  prefix: string;
  cursor: string;
  limit: number;
}

export interface ListPage {
  objects: ObjectInfo[];
  nextCursor: string;
}

export interface ByteRange {
  offset: number;
  length: number;
}

export interface ReadOptions {
  range: ByteRange;
}

export interface WriteOptions {
  contentType: string;
  contentLength: number;
  sha256: Uint8Array;
  /** Destination-side stage-and-publish. Empty means a one-shot put. */
  commitToken: string;
  /** When true, `put` stages remotely and does not publish until `commit`. */
  stageOnly: boolean;
}

export interface PutResult {
  key: string;
  bytesWritten: number;
  etag: string;
  sha256: Uint8Array;
}

export interface CopyResult {
  bytesCopied: number;
}

export interface PluginDescribe {
  apiVersion: number;
  id: string;
  kind: string;
  displayName: string;
  rpcFeatures: string[];
  scalarLimits: ScalarLimits;
  /**
   * Advertised factories (`destination`, `source`, `worker`, `contentSource`,
   * `integration`, `database`). Host still intersects with the manifest allowlist.
   */
  supportedRoles: string[];
  /**
   * Identity extras (brand, cli schema, method names, aliases).
   * Versioned JSON escape hatch; not a substitute for typed fields.
   */
  metadataJson: string;
}

/**
 * Bookclerk-as-IdP relying-party template. Plugins declare callback path and
 * client id; the host materializes `oidc_clients` rows and remains the AS.
 * `originConfigKey` is a dotted config path (e.g. integrations.audiobookshelf.base_url).
 */
export interface OidcClientTemplate {
  clientId: string;
  displayName: string;
  callbackPath: string;
  publicClient: boolean;
  defaultScopes: string[];
  issueRefreshToken: boolean;
  originConfigKey: string;
}

export interface OidcClientsOk {
  clients: OidcClientTemplate[];
}

export type OidcClientsReply =
  | { kind: "ok"; value: OidcClientsOk }
  | { kind: "err"; value: PluginError };

/** Plugin-specific extensible config. Not a substitute for typed ABI fields. */
export interface ExtensibleConfig {
  schemaVersion: number;
  mediaType: string;
  payload: Uint8Array;
}

/**
 * Opaque JSON knobs only (migration bridge). Prefer `config` for new fields.
 * OS paths, FDs, and sockets are transport-private.
 */
export interface DestinationContext {
  json: string;
  config: ExtensibleConfig;
}

export interface SourceContext {
  json: string;
  config: ExtensibleConfig;
}

export interface WorkerContext {
  jobId: string;
  json: string;
  config: ExtensibleConfig;
}

export interface ContentSourceContext {
  json: string;
  config: ExtensibleConfig;
}

export interface IntegrationContext {
  json: string;
  config: ExtensibleConfig;
}

export interface DatabaseContext {
  json: string;
  config: ExtensibleConfig;
}

/**
 * Durable command envelope (not a domain event). Command payload schema
 * version and checkpoint schema versions are independent of the plugin ABI.
 * The envelope itself is not persisted as opaque bytes across ABI majors.
 * Idempotency keys are scoped to (account, plugin, commandType) until a
 * terminal fenced outcome is committed.
 */
export interface JobInvocation {
  payloadSchemaVersion: number;
  invocationId: string;
  commandType: string;
  payloadJson: string;
  idempotencyKey: string;
  attempt: number;
  correlationId: string;
  causationId: string;
  /**
   * UTC Unix milliseconds. Host fence/lease is authoritative; this hint must
   * not outlive the fence (clock skew across VPS nodes).
   */
  deadlineUnixMs: number;
  checkpointJson: string;
  checkpointSchemaVersion: number;
  /** Resume ordinal; distinct from failure `attempt`. */
  invocationSequence: number;
  stepId: string;
}

export interface CompletedOutcome {
  message: string;
  bytesCopied: number;
}

export interface RetryableOutcome {
  message: string;
  retryAfterUnixMs: number;
}

export interface RejectedOutcome {
  message: string;
}

export interface CancelledOutcome {
  message: string;
}

export interface SuspendedOutcome {
  checkpointJson: string;
  checkpointSchemaVersion: number;
  wakeAtUnixMs: number;
}

export type JobOutcome =
  | { kind: "completed"; value: CompletedOutcome }
  | { kind: "retryable"; value: RetryableOutcome }
  | { kind: "rejected"; value: RejectedOutcome }
  | { kind: "cancelled"; value: CancelledOutcome }
  | { kind: "suspended"; value: SuspendedOutcome };

/** Domain event (not a job). Outbox-produced, at-least-once, idempotent consume. */
export interface DomainEvent {
  eventId: string;
  eventType: string;
  schemaVersion: number;
  occurredAtUnixMs: number;
  accountId: string;
  correlationId: string;
  causationId: string;
  deduplicationKey: string;
  deliveryAttempt: number;
  payload: Uint8Array;
  /** Append-only. Resume a prior EventResult.suspended. */
  checkpointJson: string;
  checkpointSchemaVersion: number;
  invocationSequence: number;
  resumePending: boolean;
  /** Append-only. Producer plugin id; empty when unknown. */
  source: string;
}

export interface EventAck {
  dummy: void;
}

export interface EventRetry {
  retryAtUnixMs: number;
  reason: string;
}

export interface EventReject {
  reason: string;
}

export interface EventDeadLetter {
  reason: string;
}

/**
 * Append-only. Mirrors job SuspendedOutcome; event handlers persist a
 * bounded checkpoint and release the process until wakeAtUnixMs. Optional
 * wake-on-matching-event fields (empty = timestamp-only).
 */
export interface EventSuspended {
  checkpointJson: string;
  checkpointSchemaVersion: number;
  wakeAtUnixMs: number;
  wakeOnEventType: string;
  wakeOnFilterJson: string;
}

export type EventResult =
  | { kind: "ack"; value: EventAck }
  | { kind: "retry"; value: EventRetry }
  | { kind: "reject"; value: EventReject }
  | { kind: "deadLetter"; value: EventDeadLetter }
  | { kind: "suspended"; value: EventSuspended };

export interface HeadOk {
  found: boolean;
  meta: ObjectMetadata;
}

export type HeadReply =
  | { kind: "ok"; value: HeadOk }
  | { kind: "err"; value: PluginError };

export type ListReply =
  | { kind: "ok"; value: ListPage }
  | { kind: "err"; value: PluginError };

export interface GetOk {
  meta: ObjectMetadata;
  body: ByteSource;
}

export type GetReply =
  | { kind: "ok"; value: GetOk }
  | { kind: "err"; value: PluginError };

export type PutReply =
  | { kind: "ok"; value: PutResult }
  | { kind: "err"; value: PluginError };

export type CopyReply =
  | { kind: "ok"; value: CopyResult }
  | { kind: "err"; value: PluginError };

export type EmptyReply =
  | { kind: "ok" }
  | { kind: "err"; value: PluginError };

export interface PullOk {
  chunk: Uint8Array;
  done: boolean;
}

export type PullReply =
  | { kind: "ok"; value: PullOk }
  | { kind: "err"; value: PluginError };

export interface OpenOk {
  meta: ObjectMetadata;
  body: ByteSource;
}

export type OpenReply =
  | { kind: "ok"; value: OpenOk }
  | { kind: "err"; value: PluginError };

export type DescribeReply =
  | { kind: "ok"; value: PluginDescribe }
  | { kind: "err"; value: PluginError };

export type DestinationReply =
  | { kind: "ok"; value: Destination }
  | { kind: "err"; value: PluginError };

export type SourceReply =
  | { kind: "ok"; value: Source }
  | { kind: "err"; value: PluginError };

export type WorkerReply =
  | { kind: "ok"; value: JobHandler }
  | { kind: "err"; value: PluginError };

export type HandleReply =
  | { kind: "ok"; value: JobOutcome }
  | { kind: "err"; value: PluginError };

export type ContentSourceReply =
  | { kind: "ok"; value: ContentSource }
  | { kind: "err"; value: PluginError };

export type IntegrationReply =
  | { kind: "ok"; value: Integration }
  | { kind: "err"; value: PluginError };

export type DatabaseReply =
  | { kind: "ok"; value: Database }
  | { kind: "err"; value: PluginError };

export type EventResultReply =
  | { kind: "ok"; value: EventResult }
  | { kind: "err"; value: PluginError };

/**
 * Migration-bridge JSON result. Frozen methods should prefer typed structs;
 * plugin-specific DTOs travel as schemaVersion + mediaType + bounded payload
 * via ExtensibleConfig, not as unbounded serde dumps.
 */
export interface JsonOk {
  json: string;
}

export type JsonReply =
  | { kind: "ok"; value: JsonOk }
  | { kind: "err"; value: PluginError };

export interface HealthOk {
  ok: boolean;
  detail: string;
}

export type HealthReply =
  | { kind: "ok"; value: HealthOk }
  | { kind: "err"; value: PluginError };

export type AdapterSessionReply =
  | { kind: "ok"; value: AdapterDatabaseSession }
  | { kind: "err"; value: PluginError };

export type GuestDatabaseReply =
  | { kind: "ok"; value: GuestDatabase }
  | { kind: "err"; value: PluginError };

/**
 * Transferred readable byte stream. The capability *is* the stream; callers
 * pull bounded windows. Abort is capability drop / RPC cancel. A failed pull
 * MUST set `err` — never a successful empty EOF.
 */
export interface ByteSource {
  /**
   * @param maxBytes - maxBytes
   * @returns result
   */
  pull(maxBytes: number): Promise<PullReply>;
}

export interface Destination {
  /**
   * @param key - key
   * @returns result
   */
  head(key: string): Promise<HeadReply>;
  /**
   * @param options - options
   * @returns result
   */
  list(options: ListOptions): Promise<ListReply>;
  /**
   * @param key - key
   * @param options - options
   * @returns result
   */
  get(key: string, options: ReadOptions): Promise<GetReply>;
  /**
   * @param key - key
   * @param body - body
   * @param options - options
   * @returns result
   */
  put(key: string, body: ByteSource, options: WriteOptions): Promise<PutReply>;
  /**
   * @param from - from
   * @param to - to
   * @returns result
   */
  copy(from: string, to: string): Promise<CopyReply>;
  /**
   * @param key - key
   * @returns result
   */
  delete(key: string): Promise<EmptyReply>;
  /**
   * Finalize a destination-side staged object. Staging itself is `put` with
   * `stageOnly = true`; bytes must stream into destination-managed temp/multipart
   * storage, never a complete local spool on host/adapter/broker/guest.
   *
   * @param key - key
   * @param commitToken - commitToken
   * @returns result
   */
  commit(key: string, commitToken: string): Promise<PutReply>;
  /**
   * @param key - key
   * @param commitToken - commitToken
   * @returns result
   */
  abortStage(key: string, commitToken: string): Promise<EmptyReply>;
}

export interface Source {
  /**
   * @param key - key
   * @returns result
   */
  open(key: string): Promise<OpenReply>;
}

export interface ProgressSink {
  /**
   * @param percent - percent
   * @param message - message
   * @returns result
   */
  report(percent: number, message: string): Promise<EmptyReply>;
}

/**
 * Transport cancellation. SDKs project this into a locally created AbortSignal
 * (AbortSignal is not a serializable Workers RPC value).
 */
export interface Cancellation {
  /**
   * @returns cancelled
   */
  poll(): Promise<boolean>;
}

export interface JobHandler {
  /**
   * @param invocation - invocation
   * @param input - input
   * @param output - output
   * @param progress - progress
   * @param cancel - cancel
   * @param database - Append-only. Host-mediated typed SQL session.
   * @param databases - Append-only. Named plugin-owned database bindings (Workers-style): each entry is an isolated database provisioned by the active adapter, separate from the Bookclerk library and from every other plugin. Empty when the manifest declares none.
   * @returns result
   */
  handle(invocation: JobInvocation, input: Source, output: Destination, progress: ProgressSink, cancel: Cancellation, database: GuestDatabase, databases: NamedDatabase[]): Promise<HandleReply>;
}

/** One named plugin-owned database binding delivered on `JobHandler.handle`. */
export interface NamedDatabase {
  /** Binding name from `plugin.toml` `capabilities.bindings.databases`. */
  name: string;
  /** Isolated typed SQL session for this binding (plugin-owned schema). */
  database: GuestDatabase;
}

/**
 * Storefront content source (not byte Source). JSON params/results are a
 * migration bridge for existing storefront DTOs.
 */
export interface ContentSource {
  /**
   * @param paramsJson - paramsJson
   * @returns result
   */
  login(paramsJson: string): Promise<JsonReply>;
  /**
   * @param paramsJson - paramsJson
   * @returns result
   */
  scan(paramsJson: string): Promise<JsonReply>;
  /**
   * @param paramsJson - paramsJson
   * @returns result
   */
  fetchTitle(paramsJson: string): Promise<JsonReply>;
  /**
   * @returns result
   */
  listAccounts(): Promise<JsonReply>;
  /**
   * @param paramsJson - paramsJson
   * @returns result
   */
  loginStart(paramsJson: string): Promise<JsonReply>;
  /**
   * @param paramsJson - paramsJson
   * @returns result
   */
  loginComplete(paramsJson: string): Promise<JsonReply>;
  /**
   * @param paramsJson - paramsJson
   * @returns result
   */
  searchCatalog(paramsJson: string): Promise<JsonReply>;
  /**
   * @param paramsJson - paramsJson
   * @returns result
   */
  expandCandidates(paramsJson: string): Promise<JsonReply>;
  /**
   * @param paramsJson - paramsJson
   * @returns result
   */
  purchaseHint(paramsJson: string): Promise<JsonReply>;
  /**
   * @param paramsJson - paramsJson
   * @returns result
   */
  listDeals(paramsJson: string): Promise<JsonReply>;
  /**
   * @returns result
   */
  health(): Promise<HealthReply>;
  /**
   * @returns result
   */
  diagnose(): Promise<JsonReply>;
  /**
   * @param paramsJson - paramsJson
   * @returns result
   */
  catalogDetail(paramsJson: string): Promise<JsonReply>;
}

export interface Integration {
  /**
   * @returns result
   */
  health(): Promise<HealthReply>;
  /**
   * @param event - event
   * @returns result
   */
  onEvent(event: DomainEvent): Promise<EventResultReply>;
  /**
   * @returns result
   */
  start(): Promise<EmptyReply>;
  /**
   * @returns result
   */
  stop(): Promise<EmptyReply>;
  /**
   * @returns result
   */
  diagnose(): Promise<JsonReply>;
  /**
   * @param paramsJson - paramsJson
   * @returns result
   */
  scanLibrary(paramsJson: string): Promise<EmptyReply>;
  /**
   * @returns result
   */
  syncListening(): Promise<JsonReply>;
  /**
   * @param paramsJson - paramsJson
   * @returns result
   */
  authenticateUser(paramsJson: string): Promise<JsonReply>;
  /**
   * @returns result
   */
  pollEvents(): Promise<JsonReply>;
}

/**
 * Identity extras carried as JSON in `describe().metadataJson`: portal auth,
 * brand colors, config option discovery, and an embedded CLI schema.
 */
export interface PluginMetadata {
  /** ABI version the guest speaks; must equal `apiVersion`. */
  apiVersion: number;
  /** Stable plugin id matching `plugin.toml` / install directory name. */
  id: string;
  /** Plugin kind: "source", "integration", "output", or "database". */
  kind: string;
  /** Human-readable name for UI lists; omitted when absent. */
  displayName?: string;
  /**
   * Declared capability method names the guest implements (e.g. "health",
   * "login", "fetchTitle").
   */
  capabilities?: string[];
  /** Portal Accounts connect mode: "oauth" or "password". */
  portalAuthMode?: string;
  /**
   * Optional env var name operators may set for password helpers; never
   * required for Accounts UI connect.
   */
  passwordEnvVar?: string;
  /** Alternate ids accepted for config / CLI targeting; omitted when empty. */
  aliases?: string[];
  /** Optional UI sort weight among peers of the same kind. */
  sortKey?: number;
  /** Portal brand colors and icon URL for Accounts / library chrome. */
  brand?: Brand;
  /** Discoverable config option groups for source UIs. */
  configOptions?: ConfigOption[];
  /** Optional embedded CLI schema (same shape as `cliDescribe`). */
  cli?: CliSchema;
}

/**
 * Portal brand crossing the RPC boundary. Distinct from `plugin.toml`
 * `logo`: `iconUrl` is the live URL or data URI the SPA renders.
 */
export interface Brand {
  /** Brand id (often matches the plugin id). */
  id: string;
  /** Display name shown next to the brand swatch. */
  name: string;
  /** Background CSS color (hex or named). */
  bg: string;
  /** Foreground CSS color for text on `bg`. */
  fg: string;
  /** Accent CSS color for highlights / CTAs. */
  accent: string;
  /** Icon URL or data URI for the portal. */
  iconUrl: string;
}

/** One discoverable config option group advertised for sources. */
export interface ConfigOption {
  /** Config key under the plugin's `config.toml` table. */
  key: string;
  /** Operator-facing label for the option group. */
  label: string;
  /** Allowed selectable values for this key. */
  values: ConfigOptionValue[];
}

/** One selectable value under a `ConfigOption`. */
export interface ConfigOptionValue {
  /** Value written to config when selected. */
  id: string;
  /** Operator-facing label for this value. */
  label: string;
}

/** Declared plugin CLI surface (`cliDescribe` / metadata `cli` / `plugin.toml`). */
export interface CliSchema {
  /** Commands exposed as `bookclerk plugins <id> <command> ...`. */
  commands: CliCommandSpec[];
}

/** One plugin CLI command under `CliSchema`. */
export interface CliCommandSpec {
  /** Command verb after the plugin id (for example "ping"). */
  name: string;
  /** Short help text for `--help`; omitted when absent. */
  about?: string;
  /** Argument / flag specs for this command (default empty). */
  args?: CliArgSpec[];
}

/** One CLI argument or flag under a `CliCommandSpec`. */
export interface CliArgSpec {
  /** Internal arg name used as the key in `CliInvokeParams.args`. */
  name: string;
  /** Long flag without leading dashes (e.g. "message" -> `--message`). */
  long?: string;
  /** Optional short flag character (e.g. "m" -> `-m`). */
  short?: string;
  /** Parsed value kind (default "string"). */
  kind?: CliArgKind;
  /** When true, the host rejects invoke if the arg is missing. */
  required?: boolean;
  /** Default string form when the operator omits the arg. */
  default?: string;
  /** Help text for this arg; omitted when absent. */
  about?: string;
  /** When true, the arg is positional rather than a flagged option. */
  positional?: boolean;
}

/** Params JSON for `cliInvoke`. */
export interface CliInvokeParams {
  /** Command name matching a `CliCommandSpec.name`. */
  command: string;
  /** Named argument values (keys match `CliArgSpec.name`; default `{}`). */
  args?: JsonValue;
}

/** Result JSON for `cliInvoke`. */
export interface CliInvokeResult {
  /** Process-style exit code (0 = success). */
  exitCode: number;
  /** Captured standard output text. */
  stdout: string;
  /** Captured standard error text. */
  stderr: string;
  /** Optional structured payload for machine consumers; omitted when absent. */
  json: JsonValue;
}

/**
 * Author-facing database adapter configuration carried in
 * `DatabaseContext.config` (mediaType
 * `application/vnd.bookclerk.db-adapter-config+json`). This is the generic
 * bootstrap mechanism for third-party adapters: the operator's granted
 * `[database.<id>]` table plus the scoped writable data dir. First-party
 * host-managed adapters receive host-private connect params instead.
 */
export interface DatabaseAdapterConfig {
  /** Scoped writable directory for this plugin (`.../plugins/<id>/data`). */
  pluginDataDir: string;
  /**
   * Granted plugin settings (operator `[database.<id>]` table) as a JSON
   * object; `{}` when the operator configured nothing.
   */
  config?: JsonValue;
  /**
   * Named plugin database binding this open serves; omitted for the primary
   * library open. Adapters advertising `DbCapabilities.pluginDatabases` must
   * serve each binding from its own isolated database.
   */
  binding?: string;
  /**
   * Host-issued opaque instance id for this (owner plugin, binding) pair.
   * Collision-resistant and stable across re-opens. Omitted for the primary
   * library open. Third-party adapters must key isolated databases on this
   * value rather than `binding` alone (two plugins may both declare `DB`).
   */
  instanceId?: string;
  /**
   * Append-only. When false, open an existing binding unit and
   * do not provision a missing one (read-only backup capture). Omitted/true
   * on older hosts means the adapter may create the unit.
   */
  provision?: boolean;
}

/**
 * JSON health payload for guests that report identity alongside liveness.
 * Role-level `health` RPCs return the typed `HealthOk` instead.
 */
export interface HealthResult {
  /** When true, the guest considers itself healthy enough for traffic. */
  ok: boolean;
  /** Plugin id echo; omitted when the guest does not duplicate identity. */
  id: string;
  /** Whether the guest believes it is enabled in config; omitted when unknown. */
  enabled: boolean;
  /** Short human detail for CLI / UI status lines; omitted when absent. */
  detail: string;
}

/**
 * JSON result of `diagnose`. Each line is printed by
 * `bookclerk plugins diagnose` / the control plane.
 */
export interface DiagnoseResult {
  /** Human-readable probe lines (default empty). */
  lines: string[];
}

/**
 * Params JSON for `ContentSource.login`. Password sources fill
 * email/password; OAuth sources use callback / external fields. There is no
 * files-dir root or library DB path -- only `pluginDataDir`.
 */
export interface LoginParams {
  /** Scoped writable directory for this plugin only (`.../plugins/<id>/data`). */
  pluginDataDir: string;
  /** Marketplace / locale for the storefront (default empty -> guest default). */
  marketplace?: string;
  /** Optional operator label stored on the account row. */
  label?: string;
  /** Account email / username for password logins; omitted for pure OAuth. */
  email?: string;
  /** Account password for password logins; never logged; omitted for OAuth. */
  password?: string;
  /** When true, overwrite an existing sealed credential for this account. */
  force?: boolean;
  /**
   * Optional bind address for OAuth callback servers (`host:port`). Ignored
   * when `callbackIpc` is set (host owns the TCP listener).
   */
  callbackBind?: string;
  /**
   * Host-owned callback IPC endpoint the guest must connect to. When set
   * (with `callbackPublicBase`), the guest must not bind a TCP listener.
   */
  callbackIpc?: string;
  /** Public base URL for the host TCP listener, e.g. `http://127.0.0.1:12345`. */
  callbackPublicBase?: string;
  /**
   * When true, use external / paste-redirect OAuth instead of a local
   * callback server.
   */
  external?: boolean;
  /** Pre-supplied OAuth redirect URL (paste flow); omitted otherwise. */
  responseUrl?: string;
  /** Prefer QR output when the guest supports it. */
  showQr?: boolean;
  /** Seconds to wait for OAuth callback capture; guest default when omitted. */
  timeoutSecs?: number;
  /** Store-specific knobs as a JSON object; guests may ignore unknowns. */
  extra?: JsonValue;
}

/** Params JSON for `ContentSource.loginStart` -- same shape as `LoginParams`. */
export type LoginStartParams = LoginParams;

/** Params JSON for `ContentSource.loginComplete`. */
export interface LoginCompleteParams {
  /** Session id previously returned by `loginStart`. */
  sessionId: string;
}

/**
 * Params JSON for `ContentSource.scan`. Host injects sealed credentials so
 * the plugin does not need a private credential store under `pluginDataDir`.
 */
export interface ScanParams {
  /** Scoped plugin data directory. */
  pluginDataDir: string;
  /** Account ids to scan; empty means all scan-enabled accounts. */
  accounts?: string[];
  /** Storefront page size (default 50). */
  pageSize?: number;
  /** When true, import podcast/episode-style rows (default true). */
  importEpisodes?: boolean;
  /** When true, import Plus/catalog entitlement titles (default true). */
  importPlusTitles?: boolean;
  /** Host-loaded credential blobs keyed by account id (JSON object). */
  credentials?: JsonValue;
}

/**
 * Params JSON for `ContentSource.fetchTitle`. Plugin writes media under
 * `cacheDir` and returns plain (DRM-free) paths. Host injects credentials;
 * guests must not open `library.db` or `master.key`.
 */
export interface FetchTitleParams {
  /** Scoped plugin data directory. */
  pluginDataDir: string;
  /** Account whose credentials apply. */
  accountId: string;
  /** Library / storefront title id to download. */
  titleId: string;
  /** Absolute path the guest should write media into (jail-granted TMPDIR). */
  cacheDir: string;
  /** Host-loaded credential blob for this account; omitted when unavailable. */
  credentials?: JsonValue;
  /** Opaque plugin table from `[sources.<id>]`. */
  sourceConfig?: JsonValue;
  /** Host acquire/download options (JSON object matching host DownloadOptions). */
  download?: JsonValue;
}

/** Params JSON for `ContentSource.searchCatalog`. */
export interface SearchCatalogParams {
  /** Free-text search query. */
  query: string;
  /** Storefront region / marketplace code (default empty -> guest default). */
  region?: string;
  /** Maximum hits to return (default 20). */
  limit?: number;
  /** 1-based page for storefronts that page (default 1). */
  page?: number;
  /** Sort key: "relevance" / "popularity" / "rating" / "title" / "author". */
  sort?: string;
  /** Optional facet ("author" / "narrator" / "series" / "genre"). */
  field?: string;
  /** Preferred content language (soft-prioritize; e.g. "en"). */
  language?: string;
}

/**
 * Params JSON for `ContentSource.expandCandidates`. Seed fields identify a
 * known title; the guest returns related catalog hits.
 */
export interface ExpandCandidatesParams {
  /** Source plugin id hint when expanding across storefronts. */
  source: string;
  /** Seed storefront product id. */
  productId: string;
  /** Seed title text. */
  title: string;
  /** Seed authors string. */
  authors: string;
  /** Seed narrators string. */
  narrators: string;
  /** Seed series name. */
  series: string;
  /** Seed series ASIN when known. */
  seriesAsin: string;
  /** Seed Amazon ASIN. */
  asin: string;
  /** Seed ISBN. */
  isbn: string;
  /** Storefront region / marketplace code. */
  region: string;
  /** Maximum candidates to return (default 20). */
  limit: number;
}

/**
 * Params JSON for `ContentSource.purchaseHint`. At least one identity field
 * (`productId` / `asin` / `isbn` / title+authors) should be set; guests may
 * return `invalid_params` when none are usable.
 */
export interface PurchaseHintParams {
  /** Storefront product id when known. */
  productId: string;
  /** Title text for fuzzy lookup. */
  title: string;
  /** Authors string for fuzzy lookup. */
  authors: string;
  /** Amazon ASIN when known. */
  asin: string;
  /** ISBN when known. */
  isbn: string;
  /** Storefront region / marketplace code. */
  region: string;
  /** When true, guests should include live price fields when available. */
  withPrice: boolean;
}

/** Params JSON for `ContentSource.listDeals`. */
export interface ListDealsParams {
  /** Optional maximum number of deals to return; guest default when omitted. */
  limit: number;
}

/** Params JSON for `ContentSource.catalogDetail`. */
export interface CatalogDetailParams {
  /** Store product id (Libro ISBN or ISBN-slug). */
  productId: string;
  /** Optional ISBN when it differs from `productId`. */
  isbn?: string;
}

/** Params JSON for `Integration.scanLibrary` (remote library sync). */
export interface ScanLibraryParams {
  /**
   * When true, force a full rescan even if the guest would otherwise
   * incremental-sync.
   */
  force: boolean;
}

/** Params JSON for `Integration.authenticateUser`. */
export interface AuthenticateUserParams {
  /** Integration username / login id. */
  username: string;
  /** Integration password; never logged by the host. */
  password: string;
}

export type DbValue =
  | { kind: "null"; value: DbType }
  | { kind: "boolean"; value: boolean }
  | { kind: "int64"; value: bigint }
  | { kind: "float64"; value: number }
  | { kind: "text"; value: string }
  | { kind: "bytes"; value: Uint8Array };

export interface DbColumn {
  name: string;
  dbType: DbType;
}

export interface DbRow {
  values: DbValue[];
}

export interface DbStatement {
  sql: string;
  parameters: DbValue[];
  kind: DbStatementKind;
  maxRows: number;
  resultSelection: DbResultSelection;
}

export interface ExecuteRequest {
  operationId: string;
  requestHash: string;
  statements: DbStatement[];
  deadlineUnixMs: number;
}

export interface SqlSpan {
  start: number;
  end: number;
}

export interface TextCollateSite {
  span: SqlSpan;
}

export interface IntegerArithSite {
  full: SqlSpan;
  lhs: SqlSpan;
  rhs: SqlSpan;
  kind: IntegerArithKind;
}

export interface PhysicalAccess {
  table: string;
  /** Empty = table presence only; "*" = projection wildcard. */
  column: string;
}

export interface ResolvedAssignment {
  table: string;
  column: string;
  dest: ResolvedSqlType;
  source: ResolvedSqlType;
}

export interface NamedSqlType {
  name: string;
  sqlType: ResolvedSqlType;
}

export interface ColumnReference {
  refTable: string;
  refColumns: string[];
}

export type OptionalColumnReference =
  | { kind: "none" }
  | { kind: "some"; value: ColumnReference };

export interface ForeignKeyConstraint {
  columns: string[];
  refTable: string;
  refColumns: string[];
}

export type TableConstraint =
  | { kind: "primaryKey"; value: string[] }
  | { kind: "unique"; value: string[] }
  | { kind: "check"; value: string }
  | { kind: "foreignKey"; value: ForeignKeyConstraint };

export interface CreateTableSchema {
  table: string;
  columns: NamedSqlType[];
  identityColumn: string;
  columnNotNull: boolean[];
  columnUnique: boolean[];
  columnPrimaryKey: boolean[];
  columnDefaults: string[];
  columnChecks: string[];
  columnReferences: OptionalColumnReference[];
  tableConstraints: TableConstraint[];
}

export interface SchemaCreate {
  schema: CreateTableSchema;
  fingerprint: string;
  noop: boolean;
}

export type SchemaAction =
  | { kind: "none" }
  | { kind: "create"; value: SchemaCreate }
  | { kind: "drop"; value: string };

export interface ResolvedStatement {
  statementHash: string;
  outputColumns: NamedSqlType[];
  physicalAccesses: PhysicalAccess[];
  assignments: ResolvedAssignment[];
  textCollateSites: TextCollateSite[];
  integerArithSites: IntegerArithSite[];
  functions: string[];
  schemaAction: SchemaAction;
}

export interface AdapterReceipt {
  guestLen: number;
  guestHash: string;
}

export interface AdapterStatement {
  sql: string;
  parameters: DbValue[];
  kind: DbStatementKind;
  maxRows: number;
  resultSelection: DbResultSelection;
  proof: ResolvedStatement;
}

export interface AdapterExecuteRequest {
  operationId: string;
  requestHash: string;
  statements: AdapterStatement[];
  deadlineUnixMs: number;
  isolation: IsolationReq;
  receipt: AdapterReceipt;
}

export interface StatementResult {
  rows: DbRow[];
  columns: DbColumn[];
  rowsAffected: number;
}

export interface DbTiming {
  attemptElapsedUs: number;
  dbExecutionUs: number;
  dbTimingSource: string;
}

export interface ExecuteReply {
  operationId: string;
  statements: StatementResult[];
  timing: DbTiming;
}

export type ExecuteResultReply =
  | { kind: "ok"; value: ExecuteReply }
  | { kind: "err"; value: PluginError };

/**
 * Semantic SQL-contract advertisement. Diagnostic engine identity is not
 * part of the capability plane — see `DbBootstrap`.
 */
export interface DbCapabilities {
  sqlContractVersion: number;
  atomicBatch: boolean;
  returning: boolean;
  affectedRows: boolean;
  schemaMigrations: boolean;
  cancellation: boolean;
  timing: boolean;
  maxBinds: number;
  maxStatements: number;
  maxResultRows: number;
  maxPayloadBytes: number;
  maxResultBytes: number;
  maxCellBytes: number;
  maxRequestBytes: number;
  maxAtomicResultBytes: number;
  /**
   * Append-only. Adapter can open additional isolated sessions
   * for plugin-owned database bindings (per-binding file / schema / database).
   */
  pluginDatabases: boolean;
  /**
   * Maximum arguments in one physical function call after adapter hiding
   * (nested json_object / min / max / coalesce). `0` is unspecified.
   */
  maxFunctionArgs: number;
  /** Maximum columns in one CREATE TABLE / result row. `0` is unspecified. */
  maxSchemaColumns: number;
  /**
   * Maximum UTF-8 bytes of a BookclerkSQL LIKE pattern value (literals and
   * TEXT binds). Adapters that expand LIKE into GLOB must advertise a
   * conservative value that still fits the physical pattern cap. `0` is
   * unspecified.
   */
  maxPatternBytes: number;
  /**
   * Maximum UTF-8 bytes of sqlite-family lowered SQL the adapter can realize
   * for one statement (INTEGER overflow wraps, LIKE→GLOB, NULLIF, INSERT OR
   * IGNORE, query LIMIT wrap, bytes-placeholder expansion). `0` is
   * unspecified: the host does not enforce a lowered-size ceiling. First-party
   * D1 advertises 100000. Hosts compare a standardized Bookclerk lowering
   * upper bound against this number and must not branch on engine identity.
   */
  maxLoweredStatementBytes: number;
  /**
   * Adapter can expose one stable logical database state while the host
   * reads schema, rows, and identity.
   */
  consistentBackupRead: boolean;
  /**
   * Adapter can destructively replace one logical database unit so an
   * ordinary restore failure does not leave that unit partially replaced.
   */
  atomicUnitRestore: boolean;
}

export type DbBootstrapReply =
  | { kind: "ok"; value: DbBootstrap }
  | { kind: "err"; value: PluginError };

export interface DbBootstrap {
  /**
   * Diagnostic physical engine name. Hosts must not admit or generate SQL
   * from this value. Any string is valid.
   */
  engine: string;
}

export type DbCapabilitiesReply =
  | { kind: "ok"; value: DbCapabilities }
  | { kind: "err"; value: PluginError };

export interface Database {
  /**
   * @returns result
   */
  openSession(): Promise<AdapterSessionReply>;
}

/**
 * Adapter-private identity high-water (sqlite_sequence / bookclerk_identity).
 * Column names live in the canonical backup schema, not this catalog.
 */
export interface IdentityHighWater {
  table: string;
  last: bigint;
}

export type IdentityExportReply =
  | { kind: "ok"; value: IdentityHighWater[] }
  | { kind: "err"; value: PluginError };

export type UserRelationsReply =
  | { kind: "ok"; value: string[] }
  | { kind: "err"; value: PluginError };

/** Host ↔ database adapter plugin. Capability negotiation + typed execute only. */
export interface AdapterDatabaseSession {
  /**
   * @returns result
   */
  capabilities(): Promise<DbCapabilitiesReply>;
  /**
   * Canonical SQL + required structured proofs (not JSON).
   *
   * @param request - request
   * @returns result
   */
  execute(request: AdapterExecuteRequest): Promise<ExecuteResultReply>;
  /**
   * @returns result
   */
  close(): Promise<EmptyReply>;
  /**
   * Bootstrap-only SeaORM proxy metadata (not part of DbCapabilities).
   *
   * @returns result
   */
  bootstrap(): Promise<DbBootstrapReply>;
  /**
   * Snapshot/identity/restore primitives (not a SQL dialect API).
   *
   * @returns result
   */
  exportIdentity(): Promise<IdentityExportReply>;
  /**
   * @param rows - rows
   * @returns result
   */
  importIdentity(rows: IdentityHighWater[]): Promise<EmptyReply>;
  /**
   * @returns result
   */
  listUserRelations(): Promise<UserRelationsReply>;
  /**
   * @returns result
   */
  prepareUnitRestore(): Promise<EmptyReply>;
  /**
   * @param names - names
   * @returns result
   */
  dropUserRelations(names: string[]): Promise<EmptyReply>;
  /**
   * @returns result
   */
  assertRestoreConstraints(): Promise<EmptyReply>;
}

/**
 * Host-granted SQL for job plugin authors. SDK `DatabaseBinding` mirrors the
 * Cloudflare Workers D1 surface (`prepare`/`bind`/`run`/`first`/`all`/`raw`,
 * `batch`, `exec`) over this typed `execute` transport; wire types stay Cap'n
 * `ExecuteRequest`/`ExecuteReply`.
 */
export interface GuestDatabase {
  /**
   * @param request - request
   * @returns result
   */
  execute(request: ExecuteRequest): Promise<ExecuteResultReply>;
  /**
   * @returns result
   */
  close(): Promise<EmptyReply>;
}

/**
 * One already-separated BookclerkSQL operation in a plugin-owned migration.
 * `schema` is admitted DDL; `data` is admitted DML. There is no native SQL
 * escape hatch.
 */
export type PluginMigrationOp =
  | { kind: "schema"; value: string }
  | { kind: "data"; value: string };

/**
 * One plugin-owned migration application. `id` is an opaque plugin-chosen
 * stable identity (name, UUID, timestamp-like string, or digits-as-text).
 * Bookclerk assigns no order, version, or predecessor meaning to `id`.
 * Registration order is the forward sequence.
 */
export interface PluginMigration {
  id: string;
  /** at most `maxPluginMigrationOps` */
  operations: PluginMigrationOp[];
}

export interface PluginMigrationsOk {
  /**
   * At most `maxListPage` entries; aggregate id+SQL bytes at most
   * `maxPluginMigrationRegistrationBytes`; total operations at most
   * `maxPluginMigrationTotalOps`.
   */
  migrations: PluginMigration[];
}

export type PluginMigrationsReply =
  | { kind: "ok"; value: PluginMigrationsOk }
  | { kind: "err"; value: PluginError };

export interface BookclerkPlugin {
  /**
   * @returns result
   */
  describe(): Promise<DescribeReply>;
  /**
   * @param context - context
   * @returns result
   */
  destination(context: DestinationContext): Promise<DestinationReply>;
  /**
   * @param context - context
   * @returns result
   */
  source(context: SourceContext): Promise<SourceReply>;
  /**
   * @param context - context
   * @returns result
   */
  worker(context: WorkerContext): Promise<WorkerReply>;
  /**
   * @returns result
   */
  shutdown(): Promise<EmptyReply>;
  /**
   * @param context - context
   * @returns result
   */
  contentSource(context: ContentSourceContext): Promise<ContentSourceReply>;
  /**
   * @param context - context
   * @returns result
   */
  integration(context: IntegrationContext): Promise<IntegrationReply>;
  /**
   * @param context - context
   * @returns result
   */
  database(context: DatabaseContext): Promise<DatabaseReply>;
  /**
   * @returns result
   */
  cliDescribe(): Promise<JsonReply>;
  /**
   * @param paramsJson - paramsJson
   * @returns result
   */
  cliInvoke(paramsJson: string): Promise<JsonReply>;
  /**
   * Plugin-provided OIDC AS client templates. Empty list when unused.
   *
   * @returns result
   */
  oidcClients(): Promise<OidcClientsReply>;
  /**
   * Complete ordered plugin-owned migration sequence for one named binding.
   * Host calls this at binding initialization, before ordinary execute.
   * Empty list means the binding has no plugin-owned migrations.
   * Bounded by `maxListPage` / `maxPluginMigrationOps` /
   * `maxPluginMigrationTotalOps` / `maxScalarBytes` /
   * `maxPluginMigrationRegistrationBytes`.
   *
   * @param binding - binding
   * @returns result
   */
  databaseMigrations(binding: string): Promise<PluginMigrationsReply>;
}
