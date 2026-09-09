# Bookclerk plugin ABI — object-capability Workers RPC (`api_version = 2`).
#
# This file is the first supported plugin contract. Discarded development
# compatibility fields were removed and ordinals compacted. From this contract
# forward:
# - `apiVersion` / `plugin.toml` `api_version` is the single incompatible ABI
#   version. Manifest `api_version` and `describe().apiVersion` must match.
# - Named `rpcFeatures` negotiate optional facilities. Required features are
#   rejected at spawn when missing.
# - Do not reuse field, method, or union ordinals.
# - Unknown enum/union members: preserve the wire code and fail closed or
#   return typed `unsupported`. Never collapse unknown codes to `internal`.
# - Every variable-length field is bounded by the constants below.
# - Identifiers are non-empty `[a-z][a-z0-9_]{0,63}`. Timestamps are UTC
#   unix milliseconds (UInt64); zero means omitted.
# - Absent factories/methods return typed `unsupported`.
# - `describe()` advertises `supportedRoles`. The signed manifest is the host
#   allowlist of what may be invoked (kind alone is not sufficient).
#
# Authors never see transport-private capability table indexes. Public types
# are the interfaces below plus TypeScript `BookclerkPlugin`. ByteSource is
# the Cap'n Proto realization of a transferred byte ReadableStream.
#
# Method results are typed success/error unions. SDKs map `err` onto a thrown
# PluginError. Unknown future `code` strings MUST be preserved.
@0x816df58cae22db0c;

# Product ABI version (`plugin.toml` `api_version` / `describe().apiVersion`).
const apiVersion :UInt32 = 2;
# Maximum decoded size of an ordinary RPC scalar value (not a stream window).
const maxScalarBytes :UInt32 = 262144;
# Maximum bytes returned by one `ByteSource.pull` (flow-control window).
const maxStreamWindowBytes :UInt32 = 1048576;
# Maximum objects in one `Destination.list` page.
const maxListPage :UInt32 = 256;
# Maximum job / event checkpoint payload size (bytes).
const maxCheckpointBytes :UInt32 = 65536;
# Maximum plugin / account identifier length (bytes).
const maxIdentifierBytes :UInt32 = 64;
# Maximum granted config payload size (bytes).
const maxConfigPayloadBytes :UInt32 = 65536;
# Maximum decoded size of a domain-event scalar payload (not a stream).
const maxEventPayloadBytes :UInt32 = 65536;
# Plugin `databaseMigrations` is a startup-time scalar (not a stream). Count is
# `maxListPage`. Each SQL text is `maxScalarBytes`. Ops-per-migration, total
# operations across the registration, and the aggregate UTF-8 bytes of ids +
# SQL have dedicated caps so a jailed guest cannot force unbounded host
# allocation before semantic proof. `maxPluginMigrationRegistrationBytes` is
# id+SQL text only; `maxPluginMigrationTotalOps` bounds the structural object
# graph (2048: enough for realistic histories, including 256 migrations of ~8
# ops or 8 migrations at the per-migration cap, and far below 256×256).

# Maximum already-separated operations in one plugin-owned migration.
const maxPluginMigrationOps :UInt32 = 256;
# Maximum already-separated operations across one `databaseMigrations` registration.
const maxPluginMigrationTotalOps :UInt32 = 2048;
# Maximum aggregate UTF-8 bytes of plugin migration ids plus SQL in one `databaseMigrations` registration.
const maxPluginMigrationRegistrationBytes :UInt32 = 262144;

# Negotiable `rpcFeatures` wire names (see `PluginDescribe.rpcFeatures`).

# Guest honors scalar / stream-window / list-page caps (`rpc.scalarLimits`).
const featureScalarLimits :Text = "rpc.scalarLimits";
# Media moves through transferred `ByteRange` / `ByteSource` streams (`rpc.streams`).
const featureStreams :Text = "rpc.streams";
# Guest implements server-side `Destination.copy` (`storage.copy`).
const featureStorageCopy :Text = "storage.copy";

# Guest-advertised caps for `rpc.scalarLimits`; never above the file constants.
struct ScalarLimits {
  # Largest scalar the guest accepts; at most `maxScalarBytes`.
  maxScalarBytes @0 :UInt32;
  # Largest `ByteSource.pull` window; at most `maxStreamWindowBytes`.
  maxStreamWindowBytes @1 :UInt32;
  # Largest list page; at most `maxListPage`.
  maxListPage @2 :UInt32;
}

# `code` is a snake_case string (`not_found`, `invalid_cursor`, …). Unknown
# codes are forwarded as-is; SDKs surface them as PluginErrorCode.unknown
# while retaining the raw wire code.
struct PluginError {
  # Stable snake_case error code (see `PluginErrorCode`).
  code @0 :Text;
  # Human-readable detail; never contains secrets.
  message @1 :Text;
}

# Full metadata of one stored object.
struct ObjectMetadata {
  # Object key.
  key @0 :Text;
  # Object size in bytes.
  size @1 :UInt64;
  # MIME type; empty when unknown.
  contentType @2 :Text;
  # Backend entity tag; empty when unsupported.
  etag @3 :Text;
  # Raw SHA-256 digest (32 bytes) or empty when unknown.
  sha256 @4 :Data;
}

# Compact object entry in a list page.
struct ObjectInfo {
  # Object key.
  key @0 :Text;
  # Object size in bytes.
  size @1 :UInt64;
}

# Paging options for `Destination.list`.
struct ListOptions {
  # Only keys starting with this prefix; empty lists everything.
  prefix @0 :Text;
  # Opaque cursor from a previous `ListPage.nextCursor`; empty starts over.
  cursor @1 :Text;
  # Requested page size; clamped to `maxListPage`. `0` means guest default.
  limit @2 :UInt32;
}

# One page of `Destination.list` results.
struct ListPage {
  # Objects in this page, in backend order.
  objects @0 :List(ObjectInfo);
  # Cursor for the next page; empty when exhausted.
  nextCursor @1 :Text;
}

# Half-open byte window `[offset, offset + length)`.
struct ByteRange {
  # First byte offset.
  offset @0 :UInt64;
  # Number of bytes; `0` reads to the end.
  length @1 :UInt64;
}

# Options for `Destination.get`.
struct ReadOptions {
  # Byte range to read; an all-zero range reads the whole object.
  range @0 :ByteRange;
}

# Options for `Destination.put`.
struct WriteOptions {
  # MIME type to record; empty when unknown.
  contentType @0 :Text;
  # Expected body length in bytes; `0` when unknown (chunked).
  contentLength @1 :UInt64;
  # Expected raw SHA-256 digest; empty skips verification.
  sha256 @2 :Data;
  # Destination-side stage-and-publish. Empty means a one-shot put.
  commitToken @3 :Text;
  # When true, `put` stages remotely and does not publish until `commit`.
  stageOnly @4 :Bool;
}

# Summary of a stored (or committed) object.
struct PutResult {
  # Object key written.
  key @0 :Text;
  # Bytes persisted.
  bytesWritten @1 :UInt64;
  # Backend entity tag; empty when unsupported.
  etag @2 :Text;
  # Raw SHA-256 digest of the stored bytes; empty when not computed.
  sha256 @3 :Data;
}

# Summary of a server-side copy.
struct CopyResult {
  # Bytes copied.
  bytesCopied @0 :UInt64;
}

# Guest identity and negotiation surface returned by `describe()`.
struct PluginDescribe {
  # ABI version the guest speaks; must equal `apiVersion`.
  apiVersion @0 :UInt32;
  # Stable plugin id (`[a-z][a-z0-9_]{0,63}`).
  id @1 :Text;
  # Manifest kind (`source`, `integration`, `output`, `database`).
  kind @2 :Text;
  # Human-readable name for UI lists.
  displayName @3 :Text;
  # Negotiable feature names the guest supports (see `feature*` constants).
  rpcFeatures @4 :List(Text);
  # Guest caps when `rpc.scalarLimits` is advertised.
  scalarLimits @5 :ScalarLimits;
  # Advertised factories (`destination`, `source`, `worker`, `contentSource`,
  # `integration`, `database`). Host still intersects with the manifest allowlist.
  supportedRoles @6 :List(Text);
  # Identity extras (brand, cli schema, method names, aliases).
  # Versioned JSON escape hatch; not a substitute for typed fields.
  metadataJson @7 :Text;
}

# Bookclerk-as-IdP relying-party template. Plugins declare callback path and
# client id; the host materializes `oidc_clients` rows and remains the AS.
# `originConfigKey` is a dotted config path (e.g. integrations.audiobookshelf.base_url).
struct OidcClientTemplate {
  # OIDC client id the host materializes.
  clientId @0 :Text;
  # Name shown on consent / admin screens.
  displayName @1 :Text;
  # Redirect URI path relative to the integration origin.
  callbackPath @2 :Text;
  # When true, the client uses PKCE without a client secret.
  publicClient @3 :Bool;
  # Scopes granted by default.
  defaultScopes @4 :List(Text);
  # When true, the AS issues refresh tokens to this client.
  issueRefreshToken @5 :Bool;
  # Dotted config path holding the client's origin URL.
  originConfigKey @6 :Text;
}

# Success payload of `BookclerkPlugin.oidcClients`.
struct OidcClientsOk {
  # Client templates; empty when the plugin is not a relying party.
  clients @0 :List(OidcClientTemplate);
}

# Result union of `BookclerkPlugin.oidcClients`.
struct OidcClientsReply {
  union {
    # Success: OIDC client templates.
    ok @0 :OidcClientsOk;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Plugin-specific extensible config. Not a substitute for typed ABI fields.
struct ExtensibleConfig {
  # Version of `payload`'s schema, owned by the plugin.
  schemaVersion @0 :UInt32;
  # Media type of `payload` (e.g. `application/json`).
  mediaType @1 :Text;
  # Bounded encoded payload; at most `maxConfigPayloadBytes`.
  payload @2 :Data;
}

# Opaque JSON knobs only (migration bridge). Prefer `config` for new fields.
# OS paths, FDs, and sockets are transport-private.
struct DestinationContext {
  # Legacy opaque JSON knobs (migration bridge).
  json @0 :Text;
  # Granted extensible configuration.
  config @1 :ExtensibleConfig;
}

# Granted configuration for `BookclerkPlugin.source`.
struct SourceContext {
  # Legacy opaque JSON knobs (migration bridge).
  json @0 :Text;
  # Granted extensible configuration.
  config @1 :ExtensibleConfig;
}

# Granted configuration for `BookclerkPlugin.worker`.
struct WorkerContext {
  # Host job id this handler serves.
  jobId @0 :Text;
  # Legacy opaque JSON knobs (migration bridge).
  json @1 :Text;
  # Granted extensible configuration.
  config @2 :ExtensibleConfig;
}

# Granted configuration for `BookclerkPlugin.contentSource`.
struct ContentSourceContext {
  # Legacy opaque JSON knobs (migration bridge).
  json @0 :Text;
  # Granted extensible configuration.
  config @1 :ExtensibleConfig;
}

# Granted configuration for `BookclerkPlugin.integration`.
struct IntegrationContext {
  # Legacy opaque JSON knobs (migration bridge).
  json @0 :Text;
  # Granted extensible configuration.
  config @1 :ExtensibleConfig;
}

# Granted configuration for `BookclerkPlugin.database` (see `DatabaseAdapterConfig`).
struct DatabaseContext {
  # Legacy opaque JSON knobs (migration bridge).
  json @0 :Text;
  # Granted extensible configuration.
  config @1 :ExtensibleConfig;
}

# Durable command envelope (not a domain event). Command payload schema
# version and checkpoint schema versions are independent of the plugin ABI.
# The envelope itself is not persisted as opaque bytes across ABI majors.
# Idempotency keys are scoped to (account, plugin, commandType) until a
# terminal fenced outcome is committed.
struct JobInvocation {
  # Schema version of `payloadJson`, owned by the command type.
  payloadSchemaVersion @0 :UInt32;
  # Unique id of this invocation attempt.
  invocationId @1 :Text;
  # Command type the handler dispatches on.
  commandType @2 :Text;
  # Command payload (JSON); at most `maxScalarBytes`.
  payloadJson @3 :Text;
  # Caller idempotency key scoped to (account, plugin, commandType).
  idempotencyKey @4 :Text;
  # Failure retry counter, starting at 1.
  attempt @5 :UInt32;
  # Trace correlation id; empty when none.
  correlationId @6 :Text;
  # Id of the event or command that caused this one; empty when none.
  causationId @7 :Text;
  # UTC Unix milliseconds. Host fence/lease is authoritative; this hint must
  # not outlive the fence (clock skew across VPS nodes).
  deadlineUnixMs @8 :UInt64;
  # Checkpoint persisted by a prior `SuspendedOutcome`; empty on first run.
  checkpointJson @9 :Text;
  # Schema version of `checkpointJson`.
  checkpointSchemaVersion @10 :UInt32;
  # Resume ordinal; distinct from failure `attempt`.
  invocationSequence @11 :UInt32;
  # Optional step identifier for multi-step commands.
  stepId @12 :Text;
}

# Job finished successfully.
struct CompletedOutcome {
  # Short human summary.
  message @0 :Text;
  # Bytes produced, when meaningful.
  bytesCopied @1 :UInt64;
}

# Job failed transiently; the host reschedules it.
struct RetryableOutcome {
  # Short human reason.
  message @0 :Text;
  # Earliest retry time; `0` lets the host choose.
  retryAfterUnixMs @1 :UInt64;
}

# Job failed permanently; no retry.
struct RejectedOutcome {
  # Short human reason.
  message @0 :Text;
}

# Job observed cancellation and stopped.
struct CancelledOutcome {
  # Short human note.
  message @0 :Text;
}

# Job released the process and asks to be resumed later.
struct SuspendedOutcome {
  # Bounded checkpoint to replay on resume; at most `maxCheckpointBytes`.
  checkpointJson @0 :Text;
  # Schema version of `checkpointJson`.
  checkpointSchemaVersion @1 :UInt32;
  # Earliest resume time.
  wakeAtUnixMs @2 :UInt64;
}

# Terminal or suspended result of `JobHandler.handle`.
struct JobOutcome {
  union {
    # Finished successfully.
    completed @0 :CompletedOutcome;
    # Transient failure; retry later.
    retryable @1 :RetryableOutcome;
    # Permanent failure.
    rejected @2 :RejectedOutcome;
    # Stopped on cancellation.
    cancelled @3 :CancelledOutcome;
    # Released with a checkpoint.
    suspended @4 :SuspendedOutcome;
  }
}

# Domain event (not a job). Outbox-produced, at-least-once, idempotent consume.
struct DomainEvent {
  # Unique event id (outbox row identity).
  eventId @0 :Text;
  # Dotted event type (e.g. `library.title.added`).
  eventType @1 :Text;
  # Schema version of `payload`, owned by the event type.
  schemaVersion @2 :UInt32;
  # When the producer observed the fact.
  occurredAtUnixMs @3 :UInt64;
  # Account scope; empty for host-wide events.
  accountId @4 :Text;
  # Trace correlation id; empty when none.
  correlationId @5 :Text;
  # Id of the command or event that caused this one; empty when none.
  causationId @6 :Text;
  # Consumer-side idempotency key; stable across redeliveries.
  deduplicationKey @7 :Text;
  # Delivery counter, starting at 1.
  deliveryAttempt @8 :UInt32;
  # Encoded event payload; at most `maxEventPayloadBytes`.
  payload @9 :Data;
  # Append-only. Resume a prior EventResult.suspended.
  checkpointJson @10 :Text;
  # Schema version of `checkpointJson`.
  checkpointSchemaVersion @11 :UInt32;
  # Resume ordinal; distinct from `deliveryAttempt`.
  invocationSequence @12 :UInt32;
  # True when this delivery resumes a prior suspension.
  resumePending @13 :Bool;
  # Append-only. Producer plugin id; empty when unknown.
  source @14 :Text;
}

# Event handled; the host marks it delivered.
struct EventAck {
  # Placeholder; the struct carries no data.
  dummy @0 :Void;
}

# Redeliver later.
struct EventRetry {
  # Earliest redelivery time; `0` lets the host choose.
  retryAtUnixMs @0 :UInt64;
  # Short human reason.
  reason @1 :Text;
}

# Event rejected; the host records the reason and stops delivering.
struct EventReject {
  # Short human reason.
  reason @0 :Text;
}

# Event moved to the dead-letter queue for operator review.
struct EventDeadLetter {
  # Short human reason.
  reason @0 :Text;
}

# Append-only. Mirrors job SuspendedOutcome; event handlers persist a
# bounded checkpoint and release the process until wakeAtUnixMs. Optional
# wake-on-matching-event fields (empty = timestamp-only).
struct EventSuspended {
  # Bounded checkpoint to replay on resume; at most `maxCheckpointBytes`.
  checkpointJson @0 :Text;
  # Schema version of `checkpointJson`.
  checkpointSchemaVersion @1 :UInt32;
  # Earliest resume time.
  wakeAtUnixMs @2 :UInt64;
  # Also wake when an event of this type arrives; empty disables.
  wakeOnEventType @3 :Text;
  # JSON filter applied to matching wake events; empty matches all.
  wakeOnFilterJson @4 :Text;
}

# Outcome of `Integration.onEvent`.
struct EventResult {
  union {
    # Handled.
    ack @0 :EventAck;
    # Redeliver later.
    retry @1 :EventRetry;
    # Stop delivering.
    reject @2 :EventReject;
    # Park for operator review.
    deadLetter @3 :EventDeadLetter;
    # Released with a checkpoint.
    suspended @4 :EventSuspended;
  }
}

# Success payload of `Destination.head`.
struct HeadOk {
  # Whether the object exists.
  found @0 :Bool;
  # Object metadata; meaningful only when `found`.
  meta @1 :ObjectMetadata;
}

# Result union of `Destination.head`.
struct HeadReply {
  union {
    # Success: head probe outcome.
    ok @0 :HeadOk;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Result union of `Destination.list`.
struct ListReply {
  union {
    # Success: one list page.
    ok @0 :ListPage;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Success payload of `Destination.get`.
struct GetOk {
  # Object metadata.
  meta @0 :ObjectMetadata;
  # Streamed object bytes.
  body @1 :ByteSource;
}

# Result union of `Destination.get`.
struct GetReply {
  union {
    # Success: object metadata and body stream.
    ok @0 :GetOk;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Result union of `Destination.put` / `Destination.commit`.
struct PutReply {
  union {
    # Success: stored object summary.
    ok @0 :PutResult;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Result union of `Destination.copy`.
struct CopyReply {
  union {
    # Success: copy summary.
    ok @0 :CopyResult;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Result union of `methods without a success payload`.
struct EmptyReply {
  union {
    # Success: no payload.
    ok @0 :Void;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Success payload of `ByteSource.pull`.
struct PullOk {
  # Next bytes; may be shorter than requested.
  chunk @0 :Data;
  # True when the stream is exhausted after `chunk`.
  done @1 :Bool;
}

# Result union of `ByteSource.pull`.
struct PullReply {
  union {
    # Success: one stream window.
    ok @0 :PullOk;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Success payload of `Source.open`.
struct OpenOk {
  # Object metadata.
  meta @0 :ObjectMetadata;
  # Streamed object bytes.
  body @1 :ByteSource;
}

# Result union of `Source.open`.
struct OpenReply {
  union {
    # Success: object metadata and body stream.
    ok @0 :OpenOk;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Result union of `BookclerkPlugin.describe`.
struct DescribeReply {
  union {
    # Success: plugin identity.
    ok @0 :PluginDescribe;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Result union of `BookclerkPlugin.destination`.
struct DestinationReply {
  union {
    # Success: opened `Destination` capability.
    ok @0 :Destination;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Result union of `BookclerkPlugin.source`.
struct SourceReply {
  union {
    # Success: opened `Source` capability.
    ok @0 :Source;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Result union of `BookclerkPlugin.worker`.
struct WorkerReply {
  union {
    # Success: opened `JobHandler` capability.
    ok @0 :JobHandler;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Result union of `JobHandler.handle`.
struct HandleReply {
  union {
    # Success: job outcome.
    ok @0 :JobOutcome;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Result union of `BookclerkPlugin.contentSource`.
struct ContentSourceReply {
  union {
    # Success: opened `ContentSource` capability.
    ok @0 :ContentSource;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Result union of `BookclerkPlugin.integration`.
struct IntegrationReply {
  union {
    # Success: opened `Integration` capability.
    ok @0 :Integration;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Result union of `BookclerkPlugin.database`.
struct DatabaseReply {
  union {
    # Success: opened `Database` capability.
    ok @0 :Database;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Result union of `Integration.onEvent`.
struct EventResultReply {
  union {
    # Success: event handling outcome.
    ok @0 :EventResult;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Migration-bridge JSON result. Frozen methods should prefer typed structs;
# plugin-specific DTOs travel as schemaVersion + mediaType + bounded payload
# via ExtensibleConfig, not as unbounded serde dumps.
struct JsonOk {
  # JSON text; at most `maxScalarBytes`.
  json @0 :Text;
}

# Result union of `JSON-bridge methods`.
struct JsonReply {
  union {
    # Success: JSON text payload.
    ok @0 :JsonOk;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Typed liveness report.
struct HealthOk {
  # True when the guest is healthy enough for traffic.
  ok @0 :Bool;
  # Short human status line; empty when none.
  detail @1 :Text;
}

# Result union of `health`.
struct HealthReply {
  union {
    # Success: health status.
    ok @0 :HealthOk;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Result union of `Database.openSession`.
struct AdapterSessionReply {
  union {
    # Success: opened `AdapterDatabaseSession` capability.
    ok @0 :AdapterDatabaseSession;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Result union of `guest database opens`.
struct GuestDatabaseReply {
  union {
    # Success: opened `GuestDatabase` capability.
    ok @0 :GuestDatabase;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Transferred readable byte stream. The capability *is* the stream; callers
# pull bounded windows. Abort is capability drop / RPC cancel. A failed pull
# MUST set `err` — never a successful empty EOF.
interface ByteSource {
  # Pull the next window of bytes. `done = true` on the final chunk.
  pull @0 (
      maxBytes :UInt32  # Upper bound for this window; at most `maxStreamWindowBytes`.
  ) -> (result :PullReply);
}

# Object store the host writes acquired media into (`output.*` plugins).
# Keys are relative object paths; the plugin owns the physical layout.
interface Destination {
  # Metadata probe. `found = false` is a success, not `not_found`.
  head @0 (
      key :Text  # Object key to probe.
  ) -> (result :HeadReply);
  # Page through objects under a prefix; at most `maxListPage` per page.
  list @1 (
      options :ListOptions  # Prefix, cursor, and page size.
  ) -> (result :ListReply);
  # Open an object (or a byte range of it) for reading.
  get @2 (
      key :Text,  # Object key to read.
      options :ReadOptions  # Optional byte range.
  ) -> (result :GetReply);
  # Store an object from a transferred byte stream.
  put @3 (
      key :Text,  # Object key to write.
      body :ByteSource,  # Streamed object bytes.
      options :WriteOptions  # Content type, length, digest, staging.
  ) -> (result :PutReply);
  # Server-side copy (requires `storage.copy`).
  copy @4 (
      from :Text,  # Source object key.
      to :Text  # Destination object key.
  ) -> (result :CopyReply);
  # Remove an object; deleting a missing key is a success.
  delete @5 (
      key :Text  # Object key to remove.
  ) -> (result :EmptyReply);
  # Finalize a destination-side staged object. Staging itself is `put` with
  # `stageOnly = true`; bytes must stream into destination-managed temp/multipart
  # storage, never a complete local spool on host/adapter/broker/guest.
  commit @6 (
      key :Text,  # Object key that was staged.
      commitToken :Text  # Token from `WriteOptions.commitToken`.
  ) -> (result :PutReply);
  # Discard a staged object without publishing it.
  abortStage @7 (
      key :Text,  # Object key that was staged.
      commitToken :Text  # Token from `WriteOptions.commitToken`.
  ) -> (result :EmptyReply);
}

# Read-only byte source for job inputs (not a storefront).
interface Source {
  # Open an object for streaming reads.
  open @0 (
      key :Text  # Object key to open.
  ) -> (result :OpenReply);
}

# Host-side progress reporter handed to job handlers.
interface ProgressSink {
  # Report progress; the host coalesces frequent updates.
  report @0 (
      percent :Float32,  # Completion in `[0, 100]`.
      message :Text  # Short human-readable status line.
  ) -> (result :EmptyReply);
}

# Transport cancellation. SDKs project this into a locally created AbortSignal
# (AbortSignal is not a serializable Workers RPC value).
interface Cancellation {
  # Non-blocking check; `true` once the host has fenced the invocation.
  poll @0 () -> (cancelled :Bool);
}

# Job handler returned by `BookclerkPlugin.worker`; runs one durable command.
interface JobHandler {
  # Run one command invocation to a terminal or suspended outcome.
  handle @0 (
      invocation :JobInvocation,  # Durable command envelope.
      input :Source,  # Job input objects.
      output :Destination,  # Job output object store.
      progress :ProgressSink,  # Progress reporter.
      cancel :Cancellation,  # Host cancellation probe.
      # Append-only. Host-mediated typed SQL session.
      database :GuestDatabase,
      # Append-only. Named plugin-owned database bindings
      # (Workers-style): each entry is an isolated database provisioned by
      # the active adapter, separate from the Bookclerk library and from
      # every other plugin. Empty when the manifest declares none.
      databases :List(NamedDatabase))
      -> (result :HandleReply);
}

# One named plugin-owned database binding delivered on `JobHandler.handle`.
struct NamedDatabase {
  # Binding name from `plugin.toml` `capabilities.bindings.databases`.
  name @0 :Text;
  # Isolated typed SQL session for this binding (plugin-owned schema).
  database @1 :GuestDatabase;
}

# Storefront content source (not byte Source). JSON params/results are a
# migration bridge for existing storefront DTOs.
interface ContentSource {
  # Connect an account (password or one-shot OAuth).
  login @0 (
      paramsJson :Text  # `LoginParams` JSON.
  ) -> (result :JsonReply);
  # Sync library rows for one or more accounts.
  scan @1 (
      paramsJson :Text  # `ScanParams` JSON.
  ) -> (result :JsonReply);
  # Download and decrypt one title into `cacheDir`.
  fetchTitle @2 (
      paramsJson :Text  # `FetchTitleParams` JSON.
  ) -> (result :JsonReply);
  # Enumerate accounts the guest knows about.
  listAccounts @3 () -> (result :JsonReply);
  # Begin an interactive OAuth login; returns a session id.
  loginStart @4 (
      paramsJson :Text  # `LoginStartParams` JSON.
  ) -> (result :JsonReply);
  # Finish an interactive OAuth login started by `loginStart`.
  loginComplete @5 (
      paramsJson :Text  # `LoginCompleteParams` JSON.
  ) -> (result :JsonReply);
  # Free-text storefront catalog search.
  searchCatalog @6 (
      paramsJson :Text  # `SearchCatalogParams` JSON.
  ) -> (result :JsonReply);
  # Related-title expansion from a seed title.
  expandCandidates @7 (
      paramsJson :Text  # `ExpandCandidatesParams` JSON.
  ) -> (result :JsonReply);
  # Purchase link / price hint for one title.
  purchaseHint @8 (
      paramsJson :Text  # `PurchaseHintParams` JSON.
  ) -> (result :JsonReply);
  # Current storefront deals.
  listDeals @9 (
      paramsJson :Text  # `ListDealsParams` JSON.
  ) -> (result :JsonReply);
  # Liveness / readiness probe.
  health @10 () -> (result :HealthReply);
  # Human-readable diagnostic lines (`DiagnoseResult` JSON).
  diagnose @11 () -> (result :JsonReply);
  # Full catalog record for one product.
  catalogDetail @12 (
      paramsJson :Text  # `CatalogDetailParams` JSON.
  ) -> (result :JsonReply);
}

# Long-running integration (remote library, listening sync, IdP bridge).
interface Integration {
  # Liveness / readiness probe.
  health @0 () -> (result :HealthReply);
  # Deliver one domain event (at-least-once; must be idempotent).
  onEvent @1 (
      event :DomainEvent  # Event envelope.
  ) -> (result :EventResultReply);
  # Start background work after the host has granted bindings.
  start @2 () -> (result :EmptyReply);
  # Stop background work; the host may drop the capability afterwards.
  stop @3 () -> (result :EmptyReply);
  # Human-readable diagnostic lines (`DiagnoseResult` JSON).
  diagnose @4 () -> (result :JsonReply);
  # Re-sync the remote library.
  scanLibrary @5 (
      paramsJson :Text  # `ScanLibraryParams` JSON.
  ) -> (result :EmptyReply);
  # Push / pull listening progress.
  syncListening @6 () -> (result :JsonReply);
  # Verify remote credentials on behalf of the host.
  authenticateUser @7 (
      paramsJson :Text  # `AuthenticateUserParams` JSON.
  ) -> (result :JsonReply);
  # Drain events the remote side produced since the last poll.
  pollEvents @8 () -> (result :JsonReply);
}

#############################################################################
# JSON payload contracts
#
# The structs below never travel as Cap'n Proto bytes. They are the schema
# for the JSON payloads carried inside `Text` fields of this ABI
# (`describe().metadataJson`, `ContentSource`/`Integration` `paramsJson`,
# `cliInvoke` params/results). Field names are the literal JSON keys
# (camelCase). SDK projections (TypeScript / Python) and drift checks against
# the Rust serde types are generated from these declarations by
# `scripts/gen-plugin-abi.py`.
#############################################################################

# Marks a JSON payload field that must be present (no default).
annotation required @0xab302cbc0dbdd123 (field) :Void;
# Marks a Text-typed field whose JSON value is an arbitrary JSON value or
# object (projected as a loose JSON type, not a string).
annotation jsonValue @0xe691f0f5a4b30449 (field) :Void;
# Marks an enum whose JSON wire strings are the snake_case form of the
# enumerant names (`payloadTooLarge` -> "payload_too_large").
annotation jsonEnum @0xba425910028861ab (enum) :Void;

# Stable `PluginError.code` strings. Unknown future codes are forwarded
# as-is; SDKs surface them as a local `unknown` while keeping the raw wire
# code.
enum PluginErrorCode $jsonEnum {
  # Request params failed validation or are missing required fields.
  invalidParams @0;
  # Caller is not authenticated for this method (credentials / token).
  unauthorized @1;
  # Caller is authenticated but not allowed to perform this operation.
  forbidden @2;
  # Requested account, object key, session, or row does not exist.
  notFound @3;
  # Backend or dependency is temporarily unreachable (store API, DB, ...).
  unavailable @4;
  # Method or capability is not implemented by this guest.
  unsupported @5;
  # Unexpected guest or host failure; see `PluginError.message`.
  internal @6;
  # A scalar RPC value exceeded `maxScalarBytes`.
  payloadTooLarge @7;
  # The invocation deadline elapsed before the call completed.
  deadlineExceeded @8;
  # List cursor is missing, stale, or not from this backend.
  invalidCursor @9;
  # The invocation was cancelled (host fence / guest abort).
  cancelled @10;
  # The operation conflicts with current state (conditional put, ...).
  conflict @11;
}

# Identity extras carried as JSON in `describe().metadataJson`: portal auth,
# brand colors, config option discovery, and an embedded CLI schema.
struct PluginMetadata {
  # ABI version the guest speaks; must equal `apiVersion`.
  apiVersion @0 :UInt32 $required;
  # Stable plugin id matching `plugin.toml` / install directory name.
  id @1 :Text $required;
  # Plugin kind: "source", "integration", "output", or "database".
  kind @2 :Text $required;
  # Human-readable name for UI lists; omitted when absent.
  displayName @3 :Text;
  # Declared capability method names the guest implements (e.g. "health",
  # "login", "fetchTitle").
  capabilities @4 :List(Text);
  # Portal Accounts connect mode: "oauth" or "password".
  portalAuthMode @5 :Text;
  # Optional env var name operators may set for password helpers; never
  # required for Accounts UI connect.
  passwordEnvVar @6 :Text;
  # Alternate ids accepted for config / CLI targeting; omitted when empty.
  aliases @7 :List(Text);
  # Optional UI sort weight among peers of the same kind.
  sortKey @8 :UInt32;
  # Portal brand colors and icon URL for Accounts / library chrome.
  brand @9 :Brand;
  # Discoverable config option groups for source UIs.
  configOptions @10 :List(ConfigOption);
  # Optional embedded CLI schema (same shape as `cliDescribe`).
  cli @11 :CliSchema;
}

# Portal brand crossing the RPC boundary. Distinct from `plugin.toml`
# `logo`: `iconUrl` is the live URL or data URI the SPA renders.
struct Brand {
  # Brand id (often matches the plugin id).
  id @0 :Text $required;
  # Display name shown next to the brand swatch.
  name @1 :Text $required;
  # Background CSS color (hex or named).
  bg @2 :Text $required;
  # Foreground CSS color for text on `bg`.
  fg @3 :Text $required;
  # Accent CSS color for highlights / CTAs.
  accent @4 :Text $required;
  # Icon URL or data URI for the portal.
  iconUrl @5 :Text $required;
}

# One discoverable config option group advertised for sources.
struct ConfigOption {
  # Config key under the plugin's `config.toml` table.
  key @0 :Text $required;
  # Operator-facing label for the option group.
  label @1 :Text $required;
  # Allowed selectable values for this key.
  values @2 :List(ConfigOptionValue) $required;
}

# One selectable value under a `ConfigOption`.
struct ConfigOptionValue {
  # Value written to config when selected.
  id @0 :Text $required;
  # Operator-facing label for this value.
  label @1 :Text $required;
}

# Declared plugin CLI surface (`cliDescribe` / metadata `cli` / `plugin.toml`).
struct CliSchema {
  # Commands exposed as `bookclerk plugins <id> <command> ...`.
  commands @0 :List(CliCommandSpec);
}

# One plugin CLI command under `CliSchema`.
struct CliCommandSpec {
  # Command verb after the plugin id (for example "ping").
  name @0 :Text $required;
  # Short help text for `--help`; omitted when absent.
  about @1 :Text;
  # Argument / flag specs for this command (default empty).
  args @2 :List(CliArgSpec);
}

# Value kind for a `CliArgSpec` (wire lowercase: "string" / "bool" / ...).
enum CliArgKind $jsonEnum {
  # Free-form string argument (default).
  string @0;
  # Boolean flag ("true" / "false").
  bool @1;
  # Integer argument.
  int @2;
  # Filesystem path argument.
  path @3;
}

# One CLI argument or flag under a `CliCommandSpec`.
struct CliArgSpec {
  # Internal arg name used as the key in `CliInvokeParams.args`.
  name @0 :Text $required;
  # Long flag without leading dashes (e.g. "message" -> `--message`).
  long @1 :Text;
  # Optional short flag character (e.g. "m" -> `-m`).
  short @2 :Text;
  # Parsed value kind (default "string").
  kind @3 :CliArgKind;
  # When true, the host rejects invoke if the arg is missing.
  required @4 :Bool;
  # Default string form when the operator omits the arg.
  default @5 :Text;
  # Help text for this arg; omitted when absent.
  about @6 :Text;
  # When true, the arg is positional rather than a flagged option.
  positional @7 :Bool;
}

# Params JSON for `cliInvoke`.
struct CliInvokeParams {
  # Command name matching a `CliCommandSpec.name`.
  command @0 :Text $required;
  # Named argument values (keys match `CliArgSpec.name`; default `{}`).
  args @1 :Text $jsonValue;
}

# Result JSON for `cliInvoke`.
struct CliInvokeResult {
  # Process-style exit code (0 = success).
  exitCode @0 :Int32;
  # Captured standard output text.
  stdout @1 :Text;
  # Captured standard error text.
  stderr @2 :Text;
  # Optional structured payload for machine consumers; omitted when absent.
  json @3 :Text $jsonValue;
}

# Author-facing database adapter configuration carried in
# `DatabaseContext.config` (mediaType
# `application/vnd.bookclerk.db-adapter-config+json`). This is the generic
# bootstrap mechanism for third-party adapters: the operator's granted
# `[database.<id>]` table plus the scoped writable data dir. First-party
# host-managed adapters receive host-private connect params instead.
struct DatabaseAdapterConfig {
  # Scoped writable directory for this plugin (`.../plugins/<id>/data`).
  pluginDataDir @0 :Text $required;
  # Granted plugin settings (operator `[database.<id>]` table) as a JSON
  # object; `{}` when the operator configured nothing.
  config @1 :Text $jsonValue;
  # Named plugin database binding this open serves; omitted for the primary
  # library open. Adapters advertising `DbCapabilities.pluginDatabases` must
  # serve each binding from its own isolated database.
  binding @2 :Text;
  # Host-issued opaque instance id for this (owner plugin, binding) pair.
  # Collision-resistant and stable across re-opens. Omitted for the primary
  # library open. Third-party adapters must key isolated databases on this
  # value rather than `binding` alone (two plugins may both declare `DB`).
  instanceId @3 :Text;
  # Append-only. When false, open an existing binding unit and
  # do not provision a missing one (read-only backup capture). Omitted/true
  # on older hosts means the adapter may create the unit.
  provision @4 :Bool;
}

# JSON health payload for guests that report identity alongside liveness.
# Role-level `health` RPCs return the typed `HealthOk` instead.
struct HealthResult {
  # When true, the guest considers itself healthy enough for traffic.
  ok @0 :Bool;
  # Plugin id echo; omitted when the guest does not duplicate identity.
  id @1 :Text;
  # Whether the guest believes it is enabled in config; omitted when unknown.
  enabled @2 :Bool;
  # Short human detail for CLI / UI status lines; omitted when absent.
  detail @3 :Text;
}

# JSON result of `diagnose`. Each line is printed by
# `bookclerk plugins diagnose` / the control plane.
struct DiagnoseResult {
  # Human-readable probe lines (default empty).
  lines @0 :List(Text);
}

# Params JSON for `ContentSource.login`. Password sources fill
# email/password; OAuth sources use callback / external fields. There is no
# files-dir root or library DB path -- only `pluginDataDir`.
struct LoginParams {
  # Scoped writable directory for this plugin only (`.../plugins/<id>/data`).
  pluginDataDir @0 :Text $required;
  # Marketplace / locale for the storefront (default empty -> guest default).
  marketplace @1 :Text;
  # Optional operator label stored on the account row.
  label @2 :Text;
  # Account email / username for password logins; omitted for pure OAuth.
  email @3 :Text;
  # Account password for password logins; never logged; omitted for OAuth.
  password @4 :Text;
  # When true, overwrite an existing sealed credential for this account.
  force @5 :Bool;
  # Optional bind address for OAuth callback servers (`host:port`). Ignored
  # when `callbackIpc` is set (host owns the TCP listener).
  callbackBind @6 :Text;
  # Host-owned callback IPC endpoint the guest must connect to. When set
  # (with `callbackPublicBase`), the guest must not bind a TCP listener.
  callbackIpc @7 :Text;
  # Public base URL for the host TCP listener, e.g. `http://127.0.0.1:12345`.
  callbackPublicBase @8 :Text;
  # When true, use external / paste-redirect OAuth instead of a local
  # callback server.
  external @9 :Bool;
  # Pre-supplied OAuth redirect URL (paste flow); omitted otherwise.
  responseUrl @10 :Text;
  # Prefer QR output when the guest supports it.
  showQr @11 :Bool;
  # Seconds to wait for OAuth callback capture; guest default when omitted.
  timeoutSecs @12 :UInt64;
  # Store-specific knobs as a JSON object; guests may ignore unknowns.
  extra @13 :Text $jsonValue;
}

# Params JSON for `ContentSource.loginStart` -- same shape as `LoginParams`.
using LoginStartParams = LoginParams;

# Params JSON for `ContentSource.loginComplete`.
struct LoginCompleteParams {
  # Session id previously returned by `loginStart`.
  sessionId @0 :Text $required;
}

# Params JSON for `ContentSource.scan`. Host injects sealed credentials so
# the plugin does not need a private credential store under `pluginDataDir`.
struct ScanParams {
  # Scoped plugin data directory.
  pluginDataDir @0 :Text $required;
  # Account ids to scan; empty means all scan-enabled accounts.
  accounts @1 :List(Text);
  # Storefront page size (default 50).
  pageSize @2 :UInt32;
  # When true, import podcast/episode-style rows (default true).
  importEpisodes @3 :Bool;
  # When true, import Plus/catalog entitlement titles (default true).
  importPlusTitles @4 :Bool;
  # Host-loaded credential blobs keyed by account id (JSON object).
  credentials @5 :Text $jsonValue;
}

# Params JSON for `ContentSource.fetchTitle`. Plugin writes media under
# `cacheDir` and returns plain (DRM-free) paths. Host injects credentials;
# guests must not open `library.db` or `master.key`.
struct FetchTitleParams {
  # Scoped plugin data directory.
  pluginDataDir @0 :Text $required;
  # Account whose credentials apply.
  accountId @1 :Text $required;
  # Library / storefront title id to download.
  titleId @2 :Text $required;
  # Absolute path the guest should write media into (jail-granted TMPDIR).
  cacheDir @3 :Text $required;
  # Host-loaded credential blob for this account; omitted when unavailable.
  credentials @4 :Text $jsonValue;
  # Opaque plugin table from `[sources.<id>]`.
  sourceConfig @5 :Text $jsonValue;
  # Host acquire/download options (JSON object matching host DownloadOptions).
  download @6 :Text $jsonValue;
}

# Params JSON for `ContentSource.searchCatalog`.
struct SearchCatalogParams {
  # Free-text search query.
  query @0 :Text $required;
  # Storefront region / marketplace code (default empty -> guest default).
  region @1 :Text;
  # Maximum hits to return (default 20).
  limit @2 :UInt32;
  # 1-based page for storefronts that page (default 1).
  page @3 :UInt32;
  # Sort key: "relevance" / "popularity" / "rating" / "title" / "author".
  sort @4 :Text;
  # Optional facet ("author" / "narrator" / "series" / "genre").
  field @5 :Text;
  # Preferred content language (soft-prioritize; e.g. "en").
  language @6 :Text;
}

# Params JSON for `ContentSource.expandCandidates`. Seed fields identify a
# known title; the guest returns related catalog hits.
struct ExpandCandidatesParams {
  # Source plugin id hint when expanding across storefronts.
  source @0 :Text;
  # Seed storefront product id.
  productId @1 :Text;
  # Seed title text.
  title @2 :Text;
  # Seed authors string.
  authors @3 :Text;
  # Seed narrators string.
  narrators @4 :Text;
  # Seed series name.
  series @5 :Text;
  # Seed series ASIN when known.
  seriesAsin @6 :Text;
  # Seed Amazon ASIN.
  asin @7 :Text;
  # Seed ISBN.
  isbn @8 :Text;
  # Storefront region / marketplace code.
  region @9 :Text;
  # Maximum candidates to return (default 20).
  limit @10 :UInt32;
}

# Params JSON for `ContentSource.purchaseHint`. At least one identity field
# (`productId` / `asin` / `isbn` / title+authors) should be set; guests may
# return `invalid_params` when none are usable.
struct PurchaseHintParams {
  # Storefront product id when known.
  productId @0 :Text;
  # Title text for fuzzy lookup.
  title @1 :Text;
  # Authors string for fuzzy lookup.
  authors @2 :Text;
  # Amazon ASIN when known.
  asin @3 :Text;
  # ISBN when known.
  isbn @4 :Text;
  # Storefront region / marketplace code.
  region @5 :Text;
  # When true, guests should include live price fields when available.
  withPrice @6 :Bool;
}

# Params JSON for `ContentSource.listDeals`.
struct ListDealsParams {
  # Optional maximum number of deals to return; guest default when omitted.
  limit @0 :UInt32;
}

# Params JSON for `ContentSource.catalogDetail`.
struct CatalogDetailParams {
  # Store product id (Libro ISBN or ISBN-slug).
  productId @0 :Text $required;
  # Optional ISBN when it differs from `productId`.
  isbn @1 :Text;
}

# Params JSON for `Integration.scanLibrary` (remote library sync).
struct ScanLibraryParams {
  # When true, force a full rescan even if the guest would otherwise
  # incremental-sync.
  force @0 :Bool;
}

# Params JSON for `Integration.authenticateUser`.
struct AuthenticateUserParams {
  # Integration username / login id.
  username @0 :Text $required;
  # Integration password; never logged by the host.
  password @1 :Text $required;
}

#############################################################################
# End of JSON payload contracts
#############################################################################

# Universal database cell/parameter domain. Engine-native arrays, enums,
# unsigned integers, and JSON text sentinels are not baseline ABI values.
enum DbType {
  # Type not known (typed NULL of unknown domain).
  unspecified @0;
  # Boolean.
  bool @1;
  # Signed 64-bit integer.
  int64 @2;
  # IEEE-754 double.
  float64 @3;
  # UTF-8 text.
  text @4;
  # Raw bytes.
  bytes @5;
}

# One typed SQL cell or bind parameter.
struct DbValue {
  union {
    # SQL NULL with its declared type.
    null @0 :DbType;
    # Boolean.
    boolean @1 :Bool;
    # Signed 64-bit integer.
    int64 @2 :Int64;
    # IEEE-754 double.
    float64 @3 :Float64;
    # UTF-8 text.
    text @4 :Text;
    # Raw bytes.
    bytes @5 :Data;
  }
}

# Result column descriptor.
struct DbColumn {
  # Column name as projected.
  name @0 :Text;
  # Declared or inferred column type.
  dbType @1 :DbType;
}

# One result row.
struct DbRow {
  # Cells in `columns` order.
  values @0 :List(DbValue);
}

# How the host classifies a statement for result handling.
enum DbStatementKind {
  # Mutation without result rows.
  execute @0;
  # Query producing rows.
  select @1;
  # Mutation with `RETURNING` rows.
  returning @2;
}

# Which part of a statement's outcome the caller wants back.
enum DbResultSelection {
  # Return nothing.
  discard @0;
  # Return only `rowsAffected`.
  affectedRows @1;
  # Return rows and columns.
  rows @2;
}

# One guest statement in an `ExecuteRequest`.
struct DbStatement {
  # BookclerkSQL text; at most `maxScalarBytes`.
  sql @0 :Text;
  # Positional bind values.
  parameters @1 :List(DbValue);
  # Statement classification.
  kind @2 :DbStatementKind;
  # Row cap for queries; `0` means adapter default.
  maxRows @3 :UInt32;
  # Which outcome parts to return.
  resultSelection @4 :DbResultSelection;
}

# Guest statement batch for `GuestDatabase.execute`; runs atomically.
struct ExecuteRequest {
  # Caller-chosen idempotency key.
  operationId @0 :Text;
  # SHA-256 hex of the idempotency-relevant request; empty when omitted.
  requestHash @1 :Text;
  # Statements in execution order; at most `maxStatements`.
  statements @2 :List(DbStatement);
  # Deadline hint; `0` means none.
  deadlineUnixMs @3 :UInt64;
}

# Host → adapter execute. GuestDatabase stays ExecuteRequest-only.
enum IsolationReq {
  # All statements in one transaction.
  atomicBatch @0;
  # Run inside a savepoint of an enclosing transaction.
  nestedSavepoint @1;
  # One stable read snapshot for the whole batch.
  consistentSnapshot @2;
}

# Resolved BookclerkSQL column type after type checking.
enum ResolvedSqlType {
  # INTEGER.
  integer @0;
  # REAL.
  real @1;
  # TEXT.
  text @2;
  # BLOB.
  blob @3;
  # BOOLEAN.
  boolean @4;
  # NULL literal type.
  null @5;
}

# Byte span in the exact canonical SQL string a proof is bound to.
struct SqlSpan {
  # Inclusive start byte offset.
  start @0 :UInt32;
  # Exclusive end byte offset.
  end @1 :UInt32;
}

# One TEXT expression the adapter must collate bytewise (`COLLATE "C"`).
struct TextCollateSite {
  # Identifier or string-literal span in canonical SQL.
  span @0 :SqlSpan;
}

# INTEGER `+` `-` `*` `abs` site lowered to overflow -> NULL.
enum IntegerArithKind {
  # `lhs + rhs`.
  add @0;
  # `lhs - rhs`.
  sub @1;
  # `lhs * rhs`.
  mul @2;
  # `abs(arg)` (`lhs` is the argument span).
  abs @3;
}

# One INTEGER arithmetic expression that must not wrap or error on overflow.
struct IntegerArithSite {
  # Full expression span (`a + b` or `abs(n)`).
  full @0 :SqlSpan;
  # Left operand (or `abs` argument).
  lhs @1 :SqlSpan;
  # Right operand (`abs` repeats `lhs`).
  rhs @2 :SqlSpan;
  # Operator.
  kind @3 :IntegerArithKind;
}

# Physical table/column access used for authorization.
struct PhysicalAccess {
  # Physical table name.
  table @0 :Text;
  # Empty = table presence only; "*" = projection wildcard.
  column @1 :Text;
}

# Destination assignment `lhs = rhs` (INSERT, UPDATE, ...).
struct ResolvedAssignment {
  # Target table.
  table @0 :Text;
  # Target column.
  column @1 :Text;
  # Declared column type.
  dest @2 :ResolvedSqlType;
  # Resolved type of the assigned expression.
  source @3 :ResolvedSqlType;
}

# Column name paired with its resolved type.
struct NamedSqlType {
  # Column name.
  name @0 :Text;
  # Resolved type.
  sqlType @1 :ResolvedSqlType;
}

# `REFERENCES` target of one column.
struct ColumnReference {
  # Referenced table.
  refTable @0 :Text;
  # Referenced columns.
  refColumns @1 :List(Text);
}

# Per-column `REFERENCES` slot in `CreateTableSchema`.
struct OptionalColumnReference {
  union {
    # Column has no reference.
    none @0 :Void;
    # Column references another table.
    some @1 :ColumnReference;
  }
}

# Table-level `FOREIGN KEY` constraint.
struct ForeignKeyConstraint {
  # Local columns.
  columns @0 :List(Text);
  # Referenced table.
  refTable @1 :Text;
  # Referenced columns.
  refColumns @2 :List(Text);
}

# One table-level constraint.
struct TableConstraint {
  union {
    # `PRIMARY KEY (...)` columns.
    primaryKey @0 :List(Text);
    # `UNIQUE (...)` columns.
    unique @1 :List(Text);
    # `CHECK (...)` expression text.
    check @2 :Text;
    # `FOREIGN KEY (...) REFERENCES ...`.
    foreignKey @3 :ForeignKeyConstraint;
  }
}

# Parsed `CREATE TABLE` (canonical SQL v1). Per-column lists align with `columns`.
struct CreateTableSchema {
  # Table name.
  table @0 :Text;
  # Columns with resolved types, in order.
  columns @1 :List(NamedSqlType);
  # `INTEGER PRIMARY KEY AUTOINCREMENT` column; empty when none.
  identityColumn @2 :Text;
  # Per-column NOT NULL flags.
  columnNotNull @3 :List(Bool);
  # Per-column UNIQUE flags.
  columnUnique @4 :List(Bool);
  # Per-column PRIMARY KEY flags.
  columnPrimaryKey @5 :List(Bool);
  # Per-column DEFAULT expression text; empty when none.
  columnDefaults @6 :List(Text);
  # Per-column CHECK expression text; empty when none.
  columnChecks @7 :List(Text);
  # Per-column REFERENCES targets.
  columnReferences @8 :List(OptionalColumnReference);
  # Table-level constraints, in order.
  tableConstraints @9 :List(TableConstraint);
}

# `CREATE TABLE` action with its durable fingerprint.
struct SchemaCreate {
  # Parsed table schema.
  schema @0 :CreateTableSchema;
  # Structured schema fingerprint (hex SHA-256).
  fingerprint @1 :Text;
  # True when the catalog already holds this exact fingerprint.
  noop @2 :Bool;
}

# CREATE/DROP action recorded on a proof.
struct SchemaAction {
  union {
    # Not DDL.
    none @0 :Void;
    # `CREATE TABLE`.
    create @1 :SchemaCreate;
    # `DROP TABLE` of the named table.
    drop @2 :Text;
  }
}

# Host-produced typed proof bound to one exact canonical statement.
struct ResolvedStatement {
  # SHA-256 hex of the exact canonical SQL this proof claims.
  statementHash @0 :Text;
  # SELECT / RETURNING / VALUES output columns in order.
  outputColumns @1 :List(NamedSqlType);
  # Physical tables/columns referenced (authorization).
  physicalAccesses @2 :List(PhysicalAccess);
  # Mutation destination assignments.
  assignments @3 :List(ResolvedAssignment);
  # TEXT expression spans needing bytewise collation.
  textCollateSites @4 :List(TextCollateSite);
  # INTEGER overflow sites (`+` `-` `*` `abs`).
  integerArithSites @5 :List(IntegerArithSite);
  # Function names invoked (folded), for authorization.
  functions @6 :List(Text);
  # DDL action + fingerprint.
  schemaAction @7 :SchemaAction;
}

# Replay receipt the host asks the adapter to persist with the batch.
struct AdapterReceipt {
  # Guest statement count inside the receipt wrap (excluding host prune/select).
  guestLen @0 :UInt32;
  # Guest `requestHash` compared on replay.
  guestHash @1 :Text;
}

# One host-lowered statement with its proof.
struct AdapterStatement {
  # Canonical SQL text.
  sql @0 :Text;
  # Positional bind values.
  parameters @1 :List(DbValue);
  # Statement classification.
  kind @2 :DbStatementKind;
  # Row cap for queries; `0` means adapter default.
  maxRows @3 :UInt32;
  # Which outcome parts to return.
  resultSelection @4 :DbResultSelection;
  # Typed proof bound to `sql`.
  proof @5 :ResolvedStatement;
}

# Host -> adapter batch: proven statements plus isolation and receipt.
struct AdapterExecuteRequest {
  # Host-chosen idempotency key.
  operationId @0 :Text;
  # SHA-256 hex of the idempotency-relevant request.
  requestHash @1 :Text;
  # Statements in execution order.
  statements @2 :List(AdapterStatement);
  # Deadline hint; `0` means none.
  deadlineUnixMs @3 :UInt64;
  # Transaction isolation the adapter must realize.
  isolation @4 :IsolationReq;
  # Replay receipt to persist; zero/empty when not required.
  receipt @5 :AdapterReceipt;
}

# Outcome of one statement.
struct StatementResult {
  # Result rows (empty unless `rows` selected).
  rows @0 :List(DbRow);
  # Result column descriptors.
  columns @1 :List(DbColumn);
  # Rows changed by a mutation.
  rowsAffected @2 :UInt64;
}

# Engine timing on `ExecuteReply`.
struct DbTiming {
  # Monotonic duration of this handler attempt (microseconds).
  attemptElapsedUs @0 :UInt64;
  # Engine-reported SQL/transaction time when available (`0` = omitted).
  dbExecutionUs @1 :UInt64;
  # How `dbExecutionUs` was measured.
  dbTimingSource @2 :Text;
}

# Success payload of `execute`.
struct ExecuteReply {
  # Echo of the request `operationId`.
  operationId @0 :Text;
  # Per-statement results in request order.
  statements @1 :List(StatementResult);
  # Engine timing.
  timing @2 :DbTiming;
}

# Result union of `execute`.
struct ExecuteResultReply {
  union {
    # Success: statement results.
    ok @0 :ExecuteReply;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Semantic SQL-contract advertisement. Diagnostic engine identity is not
# part of the capability plane — see `DbBootstrap`.
struct DbCapabilities {
  # Bookclerk SQL contract version.
  sqlContractVersion @0 :UInt32;
  # Guest can run a bounded statement list as one SQL transaction.
  atomicBatch @1 :Bool;
  # Guest SQL supports `RETURNING`.
  returning @2 :Bool;
  # Guest reports `rowsAffected`.
  affectedRows @3 :Bool;
  # Guest versions schema with a `bookclerk_schema_migrations` table.
  schemaMigrations @4 :Bool;
  # Guest honors RPC/session cancellation.
  cancellation @5 :Bool;
  # Guest can fill `DbTiming.dbExecutionUs`. Not a host connect minimum.
  timing @6 :Bool;
  # Maximum bound parameters per statement.
  maxBinds @7 :UInt32;
  # Maximum statements in one atomic batch.
  maxStatements @8 :UInt32;
  # Maximum rows a query statement may return.
  maxResultRows @9 :UInt32;
  # Maximum UTF-8 bytes of SQL plus binds per statement.
  maxPayloadBytes @10 :UInt32;
  # Maximum encoded bytes of one statement's result rows.
  maxResultBytes @11 :UInt32;
  # Maximum UTF-8 / blob bytes of one result cell.
  maxCellBytes @12 :UInt32;
  # Maximum encoded bytes of one `ExecuteRequest`.
  maxRequestBytes @13 :UInt32;
  # Maximum encoded bytes of one `ExecuteReply`.
  maxAtomicResultBytes @14 :UInt32;
  # Append-only. Adapter can open additional isolated sessions
  # for plugin-owned database bindings (per-binding file / schema / database).
  pluginDatabases @15 :Bool;
  # Maximum arguments in one physical function call after adapter hiding
  # (nested min / max / coalesce). Portable json_object is already ≤ 32
  # arguments (16 pairs), matching D1. `0` is unspecified.
  maxFunctionArgs @16 :UInt32;
  # Maximum columns in one CREATE TABLE / result row. `0` is unspecified.
  maxSchemaColumns @17 :UInt32;
  # Maximum UTF-8 bytes of a BookclerkSQL LIKE pattern value (literals and
  # TEXT binds). Adapters that expand LIKE into GLOB must advertise a
  # conservative value that still fits the physical pattern cap. `0` is
  # unspecified.
  maxPatternBytes @18 :UInt32;
  # Maximum UTF-8 bytes of sqlite-family lowered SQL the adapter can realize
  # for one statement (INTEGER overflow wraps, LIKE→GLOB, NULLIF, INSERT OR
  # IGNORE, query LIMIT wrap, bytes-placeholder expansion). `0` is
  # unspecified: the host does not enforce a lowered-size ceiling. First-party
  # D1 advertises 100000. Hosts compare a standardized Bookclerk lowering
  # upper bound against this number and must not branch on engine identity.
  maxLoweredStatementBytes @19 :UInt32;
  # Adapter can expose one stable logical database state while the host
  # reads schema, rows, and identity.
  consistentBackupRead @20 :Bool;
  # Adapter can destructively replace one logical database unit so an
  # ordinary restore failure does not leave that unit partially replaced.
  atomicUnitRestore @21 :Bool;
}

# Result union of `AdapterDatabaseSession.bootstrap`.
struct DbBootstrapReply {
  union {
    # Success: bootstrap metadata.
    ok @0 :DbBootstrap;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Bootstrap-only diagnostic metadata (not a capability).
struct DbBootstrap {
  # Diagnostic physical engine name. Hosts must not admit or generate SQL
  # from this value. Any string is valid.
  engine @0 :Text;
}

# Result union of `AdapterDatabaseSession.capabilities`.
struct DbCapabilitiesReply {
  union {
    # Success: capability advertisement.
    ok @0 :DbCapabilities;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Database adapter returned by `BookclerkPlugin.database`.
interface Database {
  # Open one adapter session (capability negotiation + typed execute).
  openSession @0 () -> (result :AdapterSessionReply);
}

# Adapter-private identity high-water (sqlite_sequence / bookclerk_identity).
# Column names live in the canonical backup schema, not this catalog.
struct IdentityHighWater {
  # Table whose identity column the mark belongs to.
  table @0 :Text;
  # Highest generated or stored value that must not be reused.
  last @1 :Int64;
}

# Result union of `AdapterDatabaseSession.exportIdentity`.
struct IdentityExportReply {
  union {
    # Success: identity high-water rows.
    ok @0 :List(IdentityHighWater);
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Result union of `AdapterDatabaseSession.listUserRelations`.
struct UserRelationsReply {
  union {
    # Success: user relation names.
    ok @0 :List(Text);
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Host ↔ database adapter plugin. Capability negotiation + typed execute only.
interface AdapterDatabaseSession {
  # Semantic SQL-contract advertisement for this session.
  capabilities @0 () -> (result :DbCapabilitiesReply);
  # Canonical SQL + required structured proofs (not JSON).
  execute @1 (
      request :AdapterExecuteRequest  # Proven statements plus isolation and receipt.
  ) -> (result :ExecuteResultReply);
  # Release the session; further calls fail.
  close @2 () -> (result :EmptyReply);
  # Bootstrap-only SeaORM proxy metadata (not part of DbCapabilities).
  bootstrap @3 () -> (result :DbBootstrapReply);
  # Snapshot/identity/restore primitives (not a SQL dialect API).
  exportIdentity @4 () -> (result :IdentityExportReply);
  # Restore identity high-water marks captured by `exportIdentity`.
  importIdentity @5 (
      rows :List(IdentityHighWater)  # High-water rows to apply.
  ) -> (result :EmptyReply);
  # Names of user relations present in the logical database unit.
  listUserRelations @6 () -> (result :UserRelationsReply);
  # Enter restore mode for one logical unit (`atomicUnitRestore`).
  prepareUnitRestore @7 () -> (result :EmptyReply);
  # Drop the named user relations during restore.
  dropUserRelations @8 (
      names :List(Text)  # Relation names from `listUserRelations`.
  ) -> (result :EmptyReply);
  # Verify constraints hold after restore rows were written.
  assertRestoreConstraints @9 () -> (result :EmptyReply);
}

# Host-granted SQL for job plugin authors. SDK `DatabaseBinding` mirrors the
# Cloudflare Workers D1 surface (`prepare`/`bind`/`run`/`first`/`all`/`raw`,
# `batch`, `exec`) over this typed `execute` transport; wire types stay Cap'n
# `ExecuteRequest`/`ExecuteReply`.
interface GuestDatabase {
  # Run one typed statement batch.
  execute @0 (
      request :ExecuteRequest  # Statements, binds, and deadline.
  ) -> (result :ExecuteResultReply);
  # Release the session; further calls fail.
  close @1 () -> (result :EmptyReply);
}

# One already-separated BookclerkSQL operation in a plugin-owned migration.
# `schema` is admitted DDL; `data` is admitted DML. There is no native SQL
# escape hatch.
struct PluginMigrationOp {
  union {
    # Admitted DDL statement text.
    schema @0 :Text;
    # Admitted DML statement text.
    data @1 :Text;
  }
}

# One plugin-owned migration application. `id` is an opaque plugin-chosen
# stable identity (name, UUID, timestamp-like string, or digits-as-text).
# Bookclerk assigns no order, version, or predecessor meaning to `id`.
# Registration order is the forward sequence.
struct PluginMigration {
  # Opaque plugin-chosen stable identity.
  id @0 :Text;
  # Operations in forward order; at most `maxPluginMigrationOps`.
  operations @1 :List(PluginMigrationOp);
}

# Success payload of `BookclerkPlugin.databaseMigrations`.
struct PluginMigrationsOk {
  # At most `maxListPage` entries; aggregate id+SQL bytes at most
  # `maxPluginMigrationRegistrationBytes`; total operations at most
  # `maxPluginMigrationTotalOps`.
  migrations @0 :List(PluginMigration);
}

# Result union of `BookclerkPlugin.databaseMigrations`.
struct PluginMigrationsReply {
  union {
    # Success: ordered migration sequence.
    ok @0 :PluginMigrationsOk;
    # Typed failure; `code` is a `PluginErrorCode` wire string.
    err @1 :PluginError;
  }
}

# Plugin bootstrap capability: the guest's root object.
interface BookclerkPlugin {
  # Identity, ABI version, negotiated features, and advertised roles.
  describe @0 () -> (result :DescribeReply);
  # Open the object-store destination role.
  destination @1 (
      context :DestinationContext  # Granted destination configuration.
  ) -> (result :DestinationReply);
  # Open the byte-source role.
  source @2 (
      context :SourceContext  # Granted source configuration.
  ) -> (result :SourceReply);
  # Open a job handler for one durable command.
  worker @3 (
      context :WorkerContext  # Job id and granted configuration.
  ) -> (result :WorkerReply);
  # Flush and release resources before the process exits.
  shutdown @4 () -> (result :EmptyReply);
  # Open the storefront content-source role.
  contentSource @5 (
      context :ContentSourceContext  # Granted storefront configuration.
  ) -> (result :ContentSourceReply);
  # Open the integration role.
  integration @6 (
      context :IntegrationContext  # Granted integration configuration.
  ) -> (result :IntegrationReply);
  # Open the database adapter role.
  database @7 (
      context :DatabaseContext  # Granted adapter configuration.
  ) -> (result :DatabaseReply);
  # Declared CLI surface (`CliSchema` JSON).
  cliDescribe @8 () -> (result :JsonReply);
  # Run one plugin CLI command (`CliInvokeParams` -> `CliInvokeResult` JSON).
  cliInvoke @9 (
      paramsJson :Text  # `CliInvokeParams` JSON.
  ) -> (result :JsonReply);
  # Plugin-provided OIDC AS client templates. Empty list when unused.
  oidcClients @10 () -> (result :OidcClientsReply);
  # Complete ordered plugin-owned migration sequence for one named binding.
  # Host calls this at binding initialization, before ordinary execute.
  # Empty list means the binding has no plugin-owned migrations.
  # Bounded by `maxListPage` / `maxPluginMigrationOps` /
  # `maxPluginMigrationTotalOps` / `maxScalarBytes` /
  # `maxPluginMigrationRegistrationBytes`.
  databaseMigrations @11 (
      binding :Text  # Binding name from `plugin.toml`.
  ) -> (result :PluginMigrationsReply);
}
