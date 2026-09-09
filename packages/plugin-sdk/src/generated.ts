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

/** Guest-advertised caps for `rpc.scalarLimits`; never above the file constants. */
export interface ScalarLimits {
  /** Largest scalar the guest accepts; at most `maxScalarBytes`. */
  maxScalarBytes: number;
  /** Largest `ByteSource.pull` window; at most `maxStreamWindowBytes`. */
  maxStreamWindowBytes: number;
  /** Largest list page; at most `maxListPage`. */
  maxListPage: number;
}

/**
 * `code` is a snake_case string (`not_found`, `invalid_cursor`, …). Unknown
 * codes are forwarded as-is; SDKs surface them as PluginErrorCode.unknown
 * while retaining the raw wire code.
 */
export interface PluginError {
  /** Stable snake_case error code (see `PluginErrorCode`). */
  code: string;
  /** Human-readable detail; never contains secrets. */
  message: string;
}

/** Full metadata of one stored object. */
export interface ObjectMetadata {
  /** Object key. */
  key: string;
  /** Object size in bytes. */
  size: number;
  /** MIME type; empty when unknown. */
  contentType: string;
  /** Backend entity tag; empty when unsupported. */
  etag: string;
  /** Raw SHA-256 digest (32 bytes) or empty when unknown. */
  sha256: Uint8Array;
}

/** Compact object entry in a list page. */
export interface ObjectInfo {
  /** Object key. */
  key: string;
  /** Object size in bytes. */
  size: number;
}

/** Paging options for `Destination.list`. */
export interface ListOptions {
  /** Only keys starting with this prefix; empty lists everything. */
  prefix: string;
  /** Opaque cursor from a previous `ListPage.nextCursor`; empty starts over. */
  cursor: string;
  /** Requested page size; clamped to `maxListPage`. `0` means guest default. */
  limit: number;
}

/** One page of `Destination.list` results. */
export interface ListPage {
  /** Objects in this page, in backend order. */
  objects: ObjectInfo[];
  /** Cursor for the next page; empty when exhausted. */
  nextCursor: string;
}

/** Half-open byte window `[offset, offset + length)`. */
export interface ByteRange {
  /** First byte offset. */
  offset: number;
  /** Number of bytes; `0` reads to the end. */
  length: number;
}

/** Options for `Destination.get`. */
export interface ReadOptions {
  /** Byte range to read; an all-zero range reads the whole object. */
  range: ByteRange;
}

/** Options for `Destination.put`. */
export interface WriteOptions {
  /** MIME type to record; empty when unknown. */
  contentType: string;
  /** Expected body length in bytes; `0` when unknown (chunked). */
  contentLength: number;
  /** Expected raw SHA-256 digest; empty skips verification. */
  sha256: Uint8Array;
  /** Destination-side stage-and-publish. Empty means a one-shot put. */
  commitToken: string;
  /** When true, `put` stages remotely and does not publish until `commit`. */
  stageOnly: boolean;
}

/** Summary of a stored (or committed) object. */
export interface PutResult {
  /** Object key written. */
  key: string;
  /** Bytes persisted. */
  bytesWritten: number;
  /** Backend entity tag; empty when unsupported. */
  etag: string;
  /** Raw SHA-256 digest of the stored bytes; empty when not computed. */
  sha256: Uint8Array;
}

/** Summary of a server-side copy. */
export interface CopyResult {
  /** Bytes copied. */
  bytesCopied: number;
}

/** Guest identity and negotiation surface returned by `describe()`. */
export interface PluginDescribe {
  /** ABI version the guest speaks; must equal `apiVersion`. */
  apiVersion: number;
  /** Stable plugin id (`[a-z][a-z0-9_]{0,63}`). */
  id: string;
  /** Manifest kind (`source`, `integration`, `output`, `database`). */
  kind: string;
  /** Human-readable name for UI lists. */
  displayName: string;
  /** Negotiable feature names the guest supports (see `feature*` constants). */
  rpcFeatures: string[];
  /** Guest caps when `rpc.scalarLimits` is advertised. */
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
  /** OIDC client id the host materializes. */
  clientId: string;
  /** Name shown on consent / admin screens. */
  displayName: string;
  /** Redirect URI path relative to the integration origin. */
  callbackPath: string;
  /** When true, the client uses PKCE without a client secret. */
  publicClient: boolean;
  /** Scopes granted by default. */
  defaultScopes: string[];
  /** When true, the AS issues refresh tokens to this client. */
  issueRefreshToken: boolean;
  /** Dotted config path holding the client's origin URL. */
  originConfigKey: string;
}

/** Success payload of `BookclerkPlugin.oidcClients`. */
export interface OidcClientsOk {
  /** Client templates; empty when the plugin is not a relying party. */
  clients: OidcClientTemplate[];
}

/** Result union of `BookclerkPlugin.oidcClients`. */
export type OidcClientsReply =
  | { kind: "ok"; value: OidcClientsOk } // Success: OIDC client templates.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Plugin-specific extensible config. Not a substitute for typed ABI fields. */
export interface ExtensibleConfig {
  /** Version of `payload`'s schema, owned by the plugin. */
  schemaVersion: number;
  /** Media type of `payload` (e.g. `application/json`). */
  mediaType: string;
  /** Bounded encoded payload; at most `maxConfigPayloadBytes`. */
  payload: Uint8Array;
}

/**
 * Opaque JSON knobs only (migration bridge). Prefer `config` for new fields.
 * OS paths, FDs, and sockets are transport-private.
 */
export interface DestinationContext {
  /** Legacy opaque JSON knobs (migration bridge). */
  json: string;
  /** Granted extensible configuration. */
  config: ExtensibleConfig;
}

/** Granted configuration for `BookclerkPlugin.source`. */
export interface SourceContext {
  /** Legacy opaque JSON knobs (migration bridge). */
  json: string;
  /** Granted extensible configuration. */
  config: ExtensibleConfig;
}

/** Granted configuration for `BookclerkPlugin.worker`. */
export interface WorkerContext {
  /** Host job id this handler serves. */
  jobId: string;
  /** Legacy opaque JSON knobs (migration bridge). */
  json: string;
  /** Granted extensible configuration. */
  config: ExtensibleConfig;
}

/** Granted configuration for `BookclerkPlugin.contentSource`. */
export interface ContentSourceContext {
  /** Legacy opaque JSON knobs (migration bridge). */
  json: string;
  /** Granted extensible configuration. */
  config: ExtensibleConfig;
}

/** Granted configuration for `BookclerkPlugin.integration`. */
export interface IntegrationContext {
  /** Legacy opaque JSON knobs (migration bridge). */
  json: string;
  /** Granted extensible configuration. */
  config: ExtensibleConfig;
}

/** Granted configuration for `BookclerkPlugin.database` (see `DatabaseAdapterConfig`). */
export interface DatabaseContext {
  /** Legacy opaque JSON knobs (migration bridge). */
  json: string;
  /** Granted extensible configuration. */
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
  /** Schema version of `payloadJson`, owned by the command type. */
  payloadSchemaVersion: number;
  /** Unique id of this invocation attempt. */
  invocationId: string;
  /** Command type the handler dispatches on. */
  commandType: string;
  /** Command payload (JSON); at most `maxScalarBytes`. */
  payloadJson: string;
  /** Caller idempotency key scoped to (account, plugin, commandType). */
  idempotencyKey: string;
  /** Failure retry counter, starting at 1. */
  attempt: number;
  /** Trace correlation id; empty when none. */
  correlationId: string;
  /** Id of the event or command that caused this one; empty when none. */
  causationId: string;
  /**
   * UTC Unix milliseconds. Host fence/lease is authoritative; this hint must
   * not outlive the fence (clock skew across VPS nodes).
   */
  deadlineUnixMs: number;
  /** Checkpoint persisted by a prior `SuspendedOutcome`; empty on first run. */
  checkpointJson: string;
  /** Schema version of `checkpointJson`. */
  checkpointSchemaVersion: number;
  /** Resume ordinal; distinct from failure `attempt`. */
  invocationSequence: number;
  /** Optional step identifier for multi-step commands. */
  stepId: string;
}

/** Job finished successfully. */
export interface CompletedOutcome {
  /** Short human summary. */
  message: string;
  /** Bytes produced, when meaningful. */
  bytesCopied: number;
}

/** Job failed transiently; the host reschedules it. */
export interface RetryableOutcome {
  /** Short human reason. */
  message: string;
  /** Earliest retry time; `0` lets the host choose. */
  retryAfterUnixMs: number;
}

/** Job failed permanently; no retry. */
export interface RejectedOutcome {
  /** Short human reason. */
  message: string;
}

/** Job observed cancellation and stopped. */
export interface CancelledOutcome {
  /** Short human note. */
  message: string;
}

/** Job released the process and asks to be resumed later. */
export interface SuspendedOutcome {
  /** Bounded checkpoint to replay on resume; at most `maxCheckpointBytes`. */
  checkpointJson: string;
  /** Schema version of `checkpointJson`. */
  checkpointSchemaVersion: number;
  /** Earliest resume time. */
  wakeAtUnixMs: number;
}

/** Terminal or suspended result of `JobHandler.handle`. */
export type JobOutcome =
  | { kind: "completed"; value: CompletedOutcome } // Finished successfully.
  | { kind: "retryable"; value: RetryableOutcome } // Transient failure; retry later.
  | { kind: "rejected"; value: RejectedOutcome } // Permanent failure.
  | { kind: "cancelled"; value: CancelledOutcome } // Stopped on cancellation.
  | { kind: "suspended"; value: SuspendedOutcome }; // Released with a checkpoint.

/** Domain event (not a job). Outbox-produced, at-least-once, idempotent consume. */
export interface DomainEvent {
  /** Unique event id (outbox row identity). */
  eventId: string;
  /** Dotted event type (e.g. `library.title.added`). */
  eventType: string;
  /** Schema version of `payload`, owned by the event type. */
  schemaVersion: number;
  /** When the producer observed the fact. */
  occurredAtUnixMs: number;
  /** Account scope; empty for host-wide events. */
  accountId: string;
  /** Trace correlation id; empty when none. */
  correlationId: string;
  /** Id of the command or event that caused this one; empty when none. */
  causationId: string;
  /** Consumer-side idempotency key; stable across redeliveries. */
  deduplicationKey: string;
  /** Delivery counter, starting at 1. */
  deliveryAttempt: number;
  /** Encoded event payload; at most `maxEventPayloadBytes`. */
  payload: Uint8Array;
  /** Append-only. Resume a prior EventResult.suspended. */
  checkpointJson: string;
  /** Schema version of `checkpointJson`. */
  checkpointSchemaVersion: number;
  /** Resume ordinal; distinct from `deliveryAttempt`. */
  invocationSequence: number;
  /** True when this delivery resumes a prior suspension. */
  resumePending: boolean;
  /** Append-only. Producer plugin id; empty when unknown. */
  source: string;
}

/** Event handled; the host marks it delivered. */
export interface EventAck {
  /** Placeholder; the struct carries no data. */
  dummy: void;
}

/** Redeliver later. */
export interface EventRetry {
  /** Earliest redelivery time; `0` lets the host choose. */
  retryAtUnixMs: number;
  /** Short human reason. */
  reason: string;
}

/** Event rejected; the host records the reason and stops delivering. */
export interface EventReject {
  /** Short human reason. */
  reason: string;
}

/** Event moved to the dead-letter queue for operator review. */
export interface EventDeadLetter {
  /** Short human reason. */
  reason: string;
}

/**
 * Append-only. Mirrors job SuspendedOutcome; event handlers persist a
 * bounded checkpoint and release the process until wakeAtUnixMs. Optional
 * wake-on-matching-event fields (empty = timestamp-only).
 */
export interface EventSuspended {
  /** Bounded checkpoint to replay on resume; at most `maxCheckpointBytes`. */
  checkpointJson: string;
  /** Schema version of `checkpointJson`. */
  checkpointSchemaVersion: number;
  /** Earliest resume time. */
  wakeAtUnixMs: number;
  /** Also wake when an event of this type arrives; empty disables. */
  wakeOnEventType: string;
  /** JSON filter applied to matching wake events; empty matches all. */
  wakeOnFilterJson: string;
}

/** Outcome of `Integration.onEvent`. */
export type EventResult =
  | { kind: "ack"; value: EventAck } // Handled.
  | { kind: "retry"; value: EventRetry } // Redeliver later.
  | { kind: "reject"; value: EventReject } // Stop delivering.
  | { kind: "deadLetter"; value: EventDeadLetter } // Park for operator review.
  | { kind: "suspended"; value: EventSuspended }; // Released with a checkpoint.

/** Success payload of `Destination.head`. */
export interface HeadOk {
  /** Whether the object exists. */
  found: boolean;
  /** Object metadata; meaningful only when `found`. */
  meta: ObjectMetadata;
}

/** Result union of `Destination.head`. */
export type HeadReply =
  | { kind: "ok"; value: HeadOk } // Success: head probe outcome.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Result union of `Destination.list`. */
export type ListReply =
  | { kind: "ok"; value: ListPage } // Success: one list page.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Success payload of `Destination.get`. */
export interface GetOk {
  /** Object metadata. */
  meta: ObjectMetadata;
  /** Streamed object bytes. */
  body: ByteSource;
}

/** Result union of `Destination.get`. */
export type GetReply =
  | { kind: "ok"; value: GetOk } // Success: object metadata and body stream.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Result union of `Destination.put` / `Destination.commit`. */
export type PutReply =
  | { kind: "ok"; value: PutResult } // Success: stored object summary.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Result union of `Destination.copy`. */
export type CopyReply =
  | { kind: "ok"; value: CopyResult } // Success: copy summary.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Result union of `methods without a success payload`. */
export type EmptyReply =
  | { kind: "ok" } // Success: no payload.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Success payload of `ByteSource.pull`. */
export interface PullOk {
  /** Next bytes; may be shorter than requested. */
  chunk: Uint8Array;
  /** True when the stream is exhausted after `chunk`. */
  done: boolean;
}

/** Result union of `ByteSource.pull`. */
export type PullReply =
  | { kind: "ok"; value: PullOk } // Success: one stream window.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Success payload of `Source.open`. */
export interface OpenOk {
  /** Object metadata. */
  meta: ObjectMetadata;
  /** Streamed object bytes. */
  body: ByteSource;
}

/** Result union of `Source.open`. */
export type OpenReply =
  | { kind: "ok"; value: OpenOk } // Success: object metadata and body stream.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Result union of `BookclerkPlugin.describe`. */
export type DescribeReply =
  | { kind: "ok"; value: PluginDescribe } // Success: plugin identity.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Result union of `BookclerkPlugin.destination`. */
export type DestinationReply =
  | { kind: "ok"; value: Destination } // Success: opened `Destination` capability.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Result union of `BookclerkPlugin.source`. */
export type SourceReply =
  | { kind: "ok"; value: Source } // Success: opened `Source` capability.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Result union of `BookclerkPlugin.worker`. */
export type WorkerReply =
  | { kind: "ok"; value: JobHandler } // Success: opened `JobHandler` capability.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Result union of `JobHandler.handle`. */
export type HandleReply =
  | { kind: "ok"; value: JobOutcome } // Success: job outcome.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Result union of `BookclerkPlugin.contentSource`. */
export type ContentSourceReply =
  | { kind: "ok"; value: ContentSource } // Success: opened `ContentSource` capability.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Result union of `BookclerkPlugin.integration`. */
export type IntegrationReply =
  | { kind: "ok"; value: Integration } // Success: opened `Integration` capability.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Result union of `BookclerkPlugin.database`. */
export type DatabaseReply =
  | { kind: "ok"; value: Database } // Success: opened `Database` capability.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Result union of `Integration.onEvent`. */
export type EventResultReply =
  | { kind: "ok"; value: EventResult } // Success: event handling outcome.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/**
 * Migration-bridge JSON result. Frozen methods should prefer typed structs;
 * plugin-specific DTOs travel as schemaVersion + mediaType + bounded payload
 * via ExtensibleConfig, not as unbounded serde dumps.
 */
export interface JsonOk {
  /** JSON text; at most `maxScalarBytes`. */
  json: string;
}

/** Result union of `JSON-bridge methods`. */
export type JsonReply =
  | { kind: "ok"; value: JsonOk } // Success: JSON text payload.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Typed liveness report. */
export interface HealthOk {
  /** True when the guest is healthy enough for traffic. */
  ok: boolean;
  /** Short human status line; empty when none. */
  detail: string;
}

/** Result union of `health`. */
export type HealthReply =
  | { kind: "ok"; value: HealthOk } // Success: health status.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Result union of `Database.openSession`. */
export type AdapterSessionReply =
  | { kind: "ok"; value: AdapterDatabaseSession } // Success: opened `AdapterDatabaseSession` capability.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Result union of `guest database opens`. */
export type GuestDatabaseReply =
  | { kind: "ok"; value: GuestDatabase } // Success: opened `GuestDatabase` capability.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/**
 * Transferred readable byte stream. The capability *is* the stream; callers
 * pull bounded windows. Abort is capability drop / RPC cancel. A failed pull
 * MUST set `err` — never a successful empty EOF.
 */
export interface ByteSource {
  /**
   * Pull the next window of bytes. `done = true` on the final chunk.
   *
   * @param maxBytes - Upper bound for this window; at most `maxStreamWindowBytes`.
   * @returns {@link PullReply}
   */
  pull(maxBytes: number): Promise<PullReply>;
}

/**
 * Object store the host writes acquired media into (`output.*` plugins).
 * Keys are relative object paths; the plugin owns the physical layout.
 */
export interface Destination {
  /**
   * Metadata probe. `found = false` is a success, not `not_found`.
   *
   * @param key - Object key to probe.
   * @returns {@link HeadReply}
   */
  head(key: string): Promise<HeadReply>;
  /**
   * Page through objects under a prefix; at most `maxListPage` per page.
   *
   * @param options - Prefix, cursor, and page size.
   * @returns {@link ListReply}
   */
  list(options: ListOptions): Promise<ListReply>;
  /**
   * Open an object (or a byte range of it) for reading.
   *
   * @param key - Object key to read.
   * @param options - Optional byte range.
   * @returns {@link GetReply}
   */
  get(key: string, options: ReadOptions): Promise<GetReply>;
  /**
   * Store an object from a transferred byte stream.
   *
   * @param key - Object key to write.
   * @param body - Streamed object bytes.
   * @param options - Content type, length, digest, staging.
   * @returns {@link PutReply}
   */
  put(key: string, body: ByteSource, options: WriteOptions): Promise<PutReply>;
  /**
   * Server-side copy (requires `storage.copy`).
   *
   * @param from - Source object key.
   * @param to - Destination object key.
   * @returns {@link CopyReply}
   */
  copy(from: string, to: string): Promise<CopyReply>;
  /**
   * Remove an object; deleting a missing key is a success.
   *
   * @param key - Object key to remove.
   * @returns {@link EmptyReply}
   */
  delete(key: string): Promise<EmptyReply>;
  /**
   * Finalize a destination-side staged object. Staging itself is `put` with
   * `stageOnly = true`; bytes must stream into destination-managed temp/multipart
   * storage, never a complete local spool on host/adapter/broker/guest.
   *
   * @param key - Object key that was staged.
   * @param commitToken - Token from `WriteOptions.commitToken`.
   * @returns {@link PutReply}
   */
  commit(key: string, commitToken: string): Promise<PutReply>;
  /**
   * Discard a staged object without publishing it.
   *
   * @param key - Object key that was staged.
   * @param commitToken - Token from `WriteOptions.commitToken`.
   * @returns {@link EmptyReply}
   */
  abortStage(key: string, commitToken: string): Promise<EmptyReply>;
}

/** Read-only byte source for job inputs (not a storefront). */
export interface Source {
  /**
   * Open an object for streaming reads.
   *
   * @param key - Object key to open.
   * @returns {@link OpenReply}
   */
  open(key: string): Promise<OpenReply>;
}

/** Host-side progress reporter handed to job handlers. */
export interface ProgressSink {
  /**
   * Report progress; the host coalesces frequent updates.
   *
   * @param percent - Completion in `[0, 100]`.
   * @param message - Short human-readable status line.
   * @returns {@link EmptyReply}
   */
  report(percent: number, message: string): Promise<EmptyReply>;
}

/**
 * Transport cancellation. SDKs project this into a locally created AbortSignal
 * (AbortSignal is not a serializable Workers RPC value).
 */
export interface Cancellation {
  /**
   * Non-blocking check; `true` once the host has fenced the invocation.
   *
   * @returns `cancelled` (boolean)
   */
  poll(): Promise<boolean>;
}

/** Job handler returned by `BookclerkPlugin.worker`; runs one durable command. */
export interface JobHandler {
  /**
   * Run one command invocation to a terminal or suspended outcome.
   *
   * @param invocation - Durable command envelope.
   * @param input - Job input objects.
   * @param output - Job output object store.
   * @param progress - Progress reporter.
   * @param cancel - Host cancellation probe.
   * @param database - Append-only. Host-mediated typed SQL session.
   * @param databases - Append-only. Named plugin-owned database bindings (Workers-style): each entry is an isolated database provisioned by the active adapter, separate from the Bookclerk library and from every other plugin. Empty when the manifest declares none.
   * @returns {@link HandleReply}
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
   * Connect an account (password or one-shot OAuth).
   *
   * @param paramsJson - `LoginParams` JSON.
   * @returns {@link JsonReply}
   */
  login(paramsJson: string): Promise<JsonReply>;
  /**
   * Sync library rows for one or more accounts.
   *
   * @param paramsJson - `ScanParams` JSON.
   * @returns {@link JsonReply}
   */
  scan(paramsJson: string): Promise<JsonReply>;
  /**
   * Download and decrypt one title into `cacheDir`.
   *
   * @param paramsJson - `FetchTitleParams` JSON.
   * @returns {@link JsonReply}
   */
  fetchTitle(paramsJson: string): Promise<JsonReply>;
  /**
   * Enumerate accounts the guest knows about.
   *
   * @returns {@link JsonReply}
   */
  listAccounts(): Promise<JsonReply>;
  /**
   * Begin an interactive OAuth login; returns a session id.
   *
   * @param paramsJson - `LoginStartParams` JSON.
   * @returns {@link JsonReply}
   */
  loginStart(paramsJson: string): Promise<JsonReply>;
  /**
   * Finish an interactive OAuth login started by `loginStart`.
   *
   * @param paramsJson - `LoginCompleteParams` JSON.
   * @returns {@link JsonReply}
   */
  loginComplete(paramsJson: string): Promise<JsonReply>;
  /**
   * Free-text storefront catalog search.
   *
   * @param paramsJson - `SearchCatalogParams` JSON.
   * @returns {@link JsonReply}
   */
  searchCatalog(paramsJson: string): Promise<JsonReply>;
  /**
   * Related-title expansion from a seed title.
   *
   * @param paramsJson - `ExpandCandidatesParams` JSON.
   * @returns {@link JsonReply}
   */
  expandCandidates(paramsJson: string): Promise<JsonReply>;
  /**
   * Purchase link / price hint for one title.
   *
   * @param paramsJson - `PurchaseHintParams` JSON.
   * @returns {@link JsonReply}
   */
  purchaseHint(paramsJson: string): Promise<JsonReply>;
  /**
   * Current storefront deals.
   *
   * @param paramsJson - `ListDealsParams` JSON.
   * @returns {@link JsonReply}
   */
  listDeals(paramsJson: string): Promise<JsonReply>;
  /**
   * Liveness / readiness probe.
   *
   * @returns {@link HealthReply}
   */
  health(): Promise<HealthReply>;
  /**
   * Human-readable diagnostic lines (`DiagnoseResult` JSON).
   *
   * @returns {@link JsonReply}
   */
  diagnose(): Promise<JsonReply>;
  /**
   * Full catalog record for one product.
   *
   * @param paramsJson - `CatalogDetailParams` JSON.
   * @returns {@link JsonReply}
   */
  catalogDetail(paramsJson: string): Promise<JsonReply>;
}

/** Long-running integration (remote library, listening sync, IdP bridge). */
export interface Integration {
  /**
   * Liveness / readiness probe.
   *
   * @returns {@link HealthReply}
   */
  health(): Promise<HealthReply>;
  /**
   * Deliver one domain event (at-least-once; must be idempotent).
   *
   * @param event - Event envelope.
   * @returns {@link EventResultReply}
   */
  onEvent(event: DomainEvent): Promise<EventResultReply>;
  /**
   * Start background work after the host has granted bindings.
   *
   * @returns {@link EmptyReply}
   */
  start(): Promise<EmptyReply>;
  /**
   * Stop background work; the host may drop the capability afterwards.
   *
   * @returns {@link EmptyReply}
   */
  stop(): Promise<EmptyReply>;
  /**
   * Human-readable diagnostic lines (`DiagnoseResult` JSON).
   *
   * @returns {@link JsonReply}
   */
  diagnose(): Promise<JsonReply>;
  /**
   * Re-sync the remote library.
   *
   * @param paramsJson - `ScanLibraryParams` JSON.
   * @returns {@link EmptyReply}
   */
  scanLibrary(paramsJson: string): Promise<EmptyReply>;
  /**
   * Push / pull listening progress.
   *
   * @returns {@link JsonReply}
   */
  syncListening(): Promise<JsonReply>;
  /**
   * Verify remote credentials on behalf of the host.
   *
   * @param paramsJson - `AuthenticateUserParams` JSON.
   * @returns {@link JsonReply}
   */
  authenticateUser(paramsJson: string): Promise<JsonReply>;
  /**
   * Drain events the remote side produced since the last poll.
   *
   * @returns {@link JsonReply}
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

/** One typed SQL cell or bind parameter. */
export type DbValue =
  | { kind: "null"; value: DbType } // SQL NULL with its declared type.
  | { kind: "boolean"; value: boolean } // Boolean.
  | { kind: "int64"; value: bigint } // Signed 64-bit integer.
  | { kind: "float64"; value: number } // IEEE-754 double.
  | { kind: "text"; value: string } // UTF-8 text.
  | { kind: "bytes"; value: Uint8Array }; // Raw bytes.

/** Result column descriptor. */
export interface DbColumn {
  /** Column name as projected. */
  name: string;
  /** Declared or inferred column type. */
  dbType: DbType;
}

/** One result row. */
export interface DbRow {
  /** Cells in `columns` order. */
  values: DbValue[];
}

/** One guest statement in an `ExecuteRequest`. */
export interface DbStatement {
  /** BookclerkSQL text; at most `maxScalarBytes`. */
  sql: string;
  /** Positional bind values. */
  parameters: DbValue[];
  /** Statement classification. */
  kind: DbStatementKind;
  /** Row cap for queries; `0` means adapter default. */
  maxRows: number;
  /** Which outcome parts to return. */
  resultSelection: DbResultSelection;
}

/** Guest statement batch for `GuestDatabase.execute`; runs atomically. */
export interface ExecuteRequest {
  /** Caller-chosen idempotency key. */
  operationId: string;
  /** SHA-256 hex of the idempotency-relevant request; empty when omitted. */
  requestHash: string;
  /** Statements in execution order; at most `maxStatements`. */
  statements: DbStatement[];
  /** Deadline hint; `0` means none. */
  deadlineUnixMs: number;
}

/** Byte span in the exact canonical SQL string a proof is bound to. */
export interface SqlSpan {
  /** Inclusive start byte offset. */
  start: number;
  /** Exclusive end byte offset. */
  end: number;
}

/** One TEXT expression the adapter must collate bytewise (`COLLATE "C"`). */
export interface TextCollateSite {
  /** Identifier or string-literal span in canonical SQL. */
  span: SqlSpan;
}

/** One INTEGER arithmetic expression that must not wrap or error on overflow. */
export interface IntegerArithSite {
  /** Full expression span (`a + b` or `abs(n)`). */
  full: SqlSpan;
  /** Left operand (or `abs` argument). */
  lhs: SqlSpan;
  /** Right operand (`abs` repeats `lhs`). */
  rhs: SqlSpan;
  /** Operator. */
  kind: IntegerArithKind;
}

/** Physical table/column access used for authorization. */
export interface PhysicalAccess {
  /** Physical table name. */
  table: string;
  /** Empty = table presence only; "*" = projection wildcard. */
  column: string;
}

/** Destination assignment `lhs = rhs` (INSERT, UPDATE, ...). */
export interface ResolvedAssignment {
  /** Target table. */
  table: string;
  /** Target column. */
  column: string;
  /** Declared column type. */
  dest: ResolvedSqlType;
  /** Resolved type of the assigned expression. */
  source: ResolvedSqlType;
}

/** Column name paired with its resolved type. */
export interface NamedSqlType {
  /** Column name. */
  name: string;
  /** Resolved type. */
  sqlType: ResolvedSqlType;
}

/** `REFERENCES` target of one column. */
export interface ColumnReference {
  /** Referenced table. */
  refTable: string;
  /** Referenced columns. */
  refColumns: string[];
}

/** Per-column `REFERENCES` slot in `CreateTableSchema`. */
export type OptionalColumnReference =
  | { kind: "none" } // Column has no reference.
  | { kind: "some"; value: ColumnReference }; // Column references another table.

/** Table-level `FOREIGN KEY` constraint. */
export interface ForeignKeyConstraint {
  /** Local columns. */
  columns: string[];
  /** Referenced table. */
  refTable: string;
  /** Referenced columns. */
  refColumns: string[];
}

/** One table-level constraint. */
export type TableConstraint =
  | { kind: "primaryKey"; value: string[] } // `PRIMARY KEY (...)` columns.
  | { kind: "unique"; value: string[] } // `UNIQUE (...)` columns.
  | { kind: "check"; value: string } // `CHECK (...)` expression text.
  | { kind: "foreignKey"; value: ForeignKeyConstraint }; // `FOREIGN KEY (...) REFERENCES ...`.

/** Parsed `CREATE TABLE` (canonical SQL v1). Per-column lists align with `columns`. */
export interface CreateTableSchema {
  /** Table name. */
  table: string;
  /** Columns with resolved types, in order. */
  columns: NamedSqlType[];
  /** `INTEGER PRIMARY KEY AUTOINCREMENT` column; empty when none. */
  identityColumn: string;
  /** Per-column NOT NULL flags. */
  columnNotNull: boolean[];
  /** Per-column UNIQUE flags. */
  columnUnique: boolean[];
  /** Per-column PRIMARY KEY flags. */
  columnPrimaryKey: boolean[];
  /** Per-column DEFAULT expression text; empty when none. */
  columnDefaults: string[];
  /** Per-column CHECK expression text; empty when none. */
  columnChecks: string[];
  /** Per-column REFERENCES targets. */
  columnReferences: OptionalColumnReference[];
  /** Table-level constraints, in order. */
  tableConstraints: TableConstraint[];
}

/** `CREATE TABLE` action with its durable fingerprint. */
export interface SchemaCreate {
  /** Parsed table schema. */
  schema: CreateTableSchema;
  /** Structured schema fingerprint (hex SHA-256). */
  fingerprint: string;
  /** True when the catalog already holds this exact fingerprint. */
  noop: boolean;
}

/** CREATE/DROP action recorded on a proof. */
export type SchemaAction =
  | { kind: "none" } // Not DDL.
  | { kind: "create"; value: SchemaCreate } // `CREATE TABLE`.
  | { kind: "drop"; value: string }; // `DROP TABLE` of the named table.

/** Host-produced typed proof bound to one exact canonical statement. */
export interface ResolvedStatement {
  /** SHA-256 hex of the exact canonical SQL this proof claims. */
  statementHash: string;
  /** SELECT / RETURNING / VALUES output columns in order. */
  outputColumns: NamedSqlType[];
  /** Physical tables/columns referenced (authorization). */
  physicalAccesses: PhysicalAccess[];
  /** Mutation destination assignments. */
  assignments: ResolvedAssignment[];
  /** TEXT expression spans needing bytewise collation. */
  textCollateSites: TextCollateSite[];
  /** INTEGER overflow sites (`+` `-` `*` `abs`). */
  integerArithSites: IntegerArithSite[];
  /** Function names invoked (folded), for authorization. */
  functions: string[];
  /** DDL action + fingerprint. */
  schemaAction: SchemaAction;
}

/** Replay receipt the host asks the adapter to persist with the batch. */
export interface AdapterReceipt {
  /** Guest statement count inside the receipt wrap (excluding host prune/select). */
  guestLen: number;
  /** Guest `requestHash` compared on replay. */
  guestHash: string;
}

/** One host-lowered statement with its proof. */
export interface AdapterStatement {
  /** Canonical SQL text. */
  sql: string;
  /** Positional bind values. */
  parameters: DbValue[];
  /** Statement classification. */
  kind: DbStatementKind;
  /** Row cap for queries; `0` means adapter default. */
  maxRows: number;
  /** Which outcome parts to return. */
  resultSelection: DbResultSelection;
  /** Typed proof bound to `sql`. */
  proof: ResolvedStatement;
}

/** Host -> adapter batch: proven statements plus isolation and receipt. */
export interface AdapterExecuteRequest {
  /** Host-chosen idempotency key. */
  operationId: string;
  /** SHA-256 hex of the idempotency-relevant request. */
  requestHash: string;
  /** Statements in execution order. */
  statements: AdapterStatement[];
  /** Deadline hint; `0` means none. */
  deadlineUnixMs: number;
  /** Transaction isolation the adapter must realize. */
  isolation: IsolationReq;
  /** Replay receipt to persist; zero/empty when not required. */
  receipt: AdapterReceipt;
}

/** Outcome of one statement. */
export interface StatementResult {
  /** Result rows (empty unless `rows` selected). */
  rows: DbRow[];
  /** Result column descriptors. */
  columns: DbColumn[];
  /** Rows changed by a mutation. */
  rowsAffected: number;
}

/** Engine timing on `ExecuteReply`. */
export interface DbTiming {
  /** Monotonic duration of this handler attempt (microseconds). */
  attemptElapsedUs: number;
  /** Engine-reported SQL/transaction time when available (`0` = omitted). */
  dbExecutionUs: number;
  /** How `dbExecutionUs` was measured. */
  dbTimingSource: string;
}

/** Success payload of `execute`. */
export interface ExecuteReply {
  /** Echo of the request `operationId`. */
  operationId: string;
  /** Per-statement results in request order. */
  statements: StatementResult[];
  /** Engine timing. */
  timing: DbTiming;
}

/** Result union of `execute`. */
export type ExecuteResultReply =
  | { kind: "ok"; value: ExecuteReply } // Success: statement results.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/**
 * Semantic SQL-contract advertisement. Diagnostic engine identity is not
 * part of the capability plane — see `DbBootstrap`.
 */
export interface DbCapabilities {
  /** Bookclerk SQL contract version. */
  sqlContractVersion: number;
  /** Guest can run a bounded statement list as one SQL transaction. */
  atomicBatch: boolean;
  /** Guest SQL supports `RETURNING`. */
  returning: boolean;
  /** Guest reports `rowsAffected`. */
  affectedRows: boolean;
  /** Guest versions schema with a `bookclerk_schema_migrations` table. */
  schemaMigrations: boolean;
  /** Guest honors RPC/session cancellation. */
  cancellation: boolean;
  /** Guest can fill `DbTiming.dbExecutionUs`. Not a host connect minimum. */
  timing: boolean;
  /** Maximum bound parameters per statement. */
  maxBinds: number;
  /** Maximum statements in one atomic batch. */
  maxStatements: number;
  /** Maximum rows a query statement may return. */
  maxResultRows: number;
  /** Maximum UTF-8 bytes of SQL plus binds per statement. */
  maxPayloadBytes: number;
  /** Maximum encoded bytes of one statement's result rows. */
  maxResultBytes: number;
  /** Maximum UTF-8 / blob bytes of one result cell. */
  maxCellBytes: number;
  /** Maximum encoded bytes of one `ExecuteRequest`. */
  maxRequestBytes: number;
  /** Maximum encoded bytes of one `ExecuteReply`. */
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

/** Result union of `AdapterDatabaseSession.bootstrap`. */
export type DbBootstrapReply =
  | { kind: "ok"; value: DbBootstrap } // Success: bootstrap metadata.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Bootstrap-only diagnostic metadata (not a capability). */
export interface DbBootstrap {
  /**
   * Diagnostic physical engine name. Hosts must not admit or generate SQL
   * from this value. Any string is valid.
   */
  engine: string;
}

/** Result union of `AdapterDatabaseSession.capabilities`. */
export type DbCapabilitiesReply =
  | { kind: "ok"; value: DbCapabilities } // Success: capability advertisement.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Database adapter returned by `BookclerkPlugin.database`. */
export interface Database {
  /**
   * Open one adapter session (capability negotiation + typed execute).
   *
   * @returns {@link AdapterSessionReply}
   */
  openSession(): Promise<AdapterSessionReply>;
}

/**
 * Adapter-private identity high-water (sqlite_sequence / bookclerk_identity).
 * Column names live in the canonical backup schema, not this catalog.
 */
export interface IdentityHighWater {
  /** Table whose identity column the mark belongs to. */
  table: string;
  /** Highest generated or stored value that must not be reused. */
  last: bigint;
}

/** Result union of `AdapterDatabaseSession.exportIdentity`. */
export type IdentityExportReply =
  | { kind: "ok"; value: IdentityHighWater[] } // Success: identity high-water rows.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Result union of `AdapterDatabaseSession.listUserRelations`. */
export type UserRelationsReply =
  | { kind: "ok"; value: string[] } // Success: user relation names.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Host ↔ database adapter plugin. Capability negotiation + typed execute only. */
export interface AdapterDatabaseSession {
  /**
   * Semantic SQL-contract advertisement for this session.
   *
   * @returns {@link DbCapabilitiesReply}
   */
  capabilities(): Promise<DbCapabilitiesReply>;
  /**
   * Canonical SQL + required structured proofs (not JSON).
   *
   * @param request - Proven statements plus isolation and receipt.
   * @returns {@link ExecuteResultReply}
   */
  execute(request: AdapterExecuteRequest): Promise<ExecuteResultReply>;
  /**
   * Release the session; further calls fail.
   *
   * @returns {@link EmptyReply}
   */
  close(): Promise<EmptyReply>;
  /**
   * Bootstrap-only SeaORM proxy metadata (not part of DbCapabilities).
   *
   * @returns {@link DbBootstrapReply}
   */
  bootstrap(): Promise<DbBootstrapReply>;
  /**
   * Snapshot/identity/restore primitives (not a SQL dialect API).
   *
   * @returns {@link IdentityExportReply}
   */
  exportIdentity(): Promise<IdentityExportReply>;
  /**
   * Restore identity high-water marks captured by `exportIdentity`.
   *
   * @param rows - High-water rows to apply.
   * @returns {@link EmptyReply}
   */
  importIdentity(rows: IdentityHighWater[]): Promise<EmptyReply>;
  /**
   * Names of user relations present in the logical database unit.
   *
   * @returns {@link UserRelationsReply}
   */
  listUserRelations(): Promise<UserRelationsReply>;
  /**
   * Enter restore mode for one logical unit (`atomicUnitRestore`).
   *
   * @returns {@link EmptyReply}
   */
  prepareUnitRestore(): Promise<EmptyReply>;
  /**
   * Drop the named user relations during restore.
   *
   * @param names - Relation names from `listUserRelations`.
   * @returns {@link EmptyReply}
   */
  dropUserRelations(names: string[]): Promise<EmptyReply>;
  /**
   * Verify constraints hold after restore rows were written.
   *
   * @returns {@link EmptyReply}
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
   * Run one typed statement batch.
   *
   * @param request - Statements, binds, and deadline.
   * @returns {@link ExecuteResultReply}
   */
  execute(request: ExecuteRequest): Promise<ExecuteResultReply>;
  /**
   * Release the session; further calls fail.
   *
   * @returns {@link EmptyReply}
   */
  close(): Promise<EmptyReply>;
}

/**
 * One already-separated BookclerkSQL operation in a plugin-owned migration.
 * `schema` is admitted DDL; `data` is admitted DML. There is no native SQL
 * escape hatch.
 */
export type PluginMigrationOp =
  | { kind: "schema"; value: string } // Admitted DDL statement text.
  | { kind: "data"; value: string }; // Admitted DML statement text.

/**
 * One plugin-owned migration application. `id` is an opaque plugin-chosen
 * stable identity (name, UUID, timestamp-like string, or digits-as-text).
 * Bookclerk assigns no order, version, or predecessor meaning to `id`.
 * Registration order is the forward sequence.
 */
export interface PluginMigration {
  /** Opaque plugin-chosen stable identity. */
  id: string;
  /** Operations in forward order; at most `maxPluginMigrationOps`. */
  operations: PluginMigrationOp[];
}

/** Success payload of `BookclerkPlugin.databaseMigrations`. */
export interface PluginMigrationsOk {
  /**
   * At most `maxListPage` entries; aggregate id+SQL bytes at most
   * `maxPluginMigrationRegistrationBytes`; total operations at most
   * `maxPluginMigrationTotalOps`.
   */
  migrations: PluginMigration[];
}

/** Result union of `BookclerkPlugin.databaseMigrations`. */
export type PluginMigrationsReply =
  | { kind: "ok"; value: PluginMigrationsOk } // Success: ordered migration sequence.
  | { kind: "err"; value: PluginError }; // Typed failure; `code` is a `PluginErrorCode` wire string.

/** Plugin bootstrap capability: the guest's root object. */
export interface BookclerkPlugin {
  /**
   * Identity, ABI version, negotiated features, and advertised roles.
   *
   * @returns {@link DescribeReply}
   */
  describe(): Promise<DescribeReply>;
  /**
   * Open the object-store destination role.
   *
   * @param context - Granted destination configuration.
   * @returns {@link DestinationReply}
   */
  destination(context: DestinationContext): Promise<DestinationReply>;
  /**
   * Open the byte-source role.
   *
   * @param context - Granted source configuration.
   * @returns {@link SourceReply}
   */
  source(context: SourceContext): Promise<SourceReply>;
  /**
   * Open a job handler for one durable command.
   *
   * @param context - Job id and granted configuration.
   * @returns {@link WorkerReply}
   */
  worker(context: WorkerContext): Promise<WorkerReply>;
  /**
   * Flush and release resources before the process exits.
   *
   * @returns {@link EmptyReply}
   */
  shutdown(): Promise<EmptyReply>;
  /**
   * Open the storefront content-source role.
   *
   * @param context - Granted storefront configuration.
   * @returns {@link ContentSourceReply}
   */
  contentSource(context: ContentSourceContext): Promise<ContentSourceReply>;
  /**
   * Open the integration role.
   *
   * @param context - Granted integration configuration.
   * @returns {@link IntegrationReply}
   */
  integration(context: IntegrationContext): Promise<IntegrationReply>;
  /**
   * Open the database adapter role.
   *
   * @param context - Granted adapter configuration.
   * @returns {@link DatabaseReply}
   */
  database(context: DatabaseContext): Promise<DatabaseReply>;
  /**
   * Declared CLI surface (`CliSchema` JSON).
   *
   * @returns {@link JsonReply}
   */
  cliDescribe(): Promise<JsonReply>;
  /**
   * Run one plugin CLI command (`CliInvokeParams` -> `CliInvokeResult` JSON).
   *
   * @param paramsJson - `CliInvokeParams` JSON.
   * @returns {@link JsonReply}
   */
  cliInvoke(paramsJson: string): Promise<JsonReply>;
  /**
   * Plugin-provided OIDC AS client templates. Empty list when unused.
   *
   * @returns {@link OidcClientsReply}
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
   * @param binding - Binding name from `plugin.toml`.
   * @returns {@link PluginMigrationsReply}
   */
  databaseMigrations(binding: string): Promise<PluginMigrationsReply>;
}
