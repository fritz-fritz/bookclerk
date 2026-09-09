"""GENERATED FILE - do not edit. Run `python3 scripts/gen-plugin-abi.py --write` after changing crates/bookclerk-plugin-abi/schema/plugin.capnp.

Author-facing Python projection of every struct, union, enum, and
interface in ``crates/bookclerk-plugin-abi/schema/plugin.capnp``.
TypedDict keys are the schema names (camelCase). Cap'n Proto unions
become ``{"kind": ..., "value": ...}`` TypedDict unions; ``Void``
members carry no ``value``. 64-bit integers are Python ``int``; ``Data``
is ``bytes``. Interfaces are ``Protocol`` classes with ``async`` methods.
"""

from __future__ import annotations

from typing import Any, Literal, NotRequired, Protocol, TypedDict, Union

JsonValue = Any
"""Arbitrary JSON value carried inside a ``$jsonValue`` ``Text`` field."""

JsonObject = dict[str, Any]
"""JSON object carried inside a ``$jsonValue`` ``Text`` field."""

PluginErrorCode = Literal[
    "invalid_params",
    "unauthorized",
    "forbidden",
    "not_found",
    "unavailable",
    "unsupported",
    "internal",
    "payload_too_large",
    "deadline_exceeded",
    "invalid_cursor",
    "cancelled",
    "conflict",
]
"""Stable `PluginError.code` strings. Unknown future codes are forwarded
as-is; SDKs surface them as a local `unknown` while keeping the raw wire
code.
"""

CliArgKind = Literal["string", "bool", "int", "path"]
"""Value kind for a `CliArgSpec` (wire lowercase: "string" / "bool" / ...)."""

DbType = Literal["unspecified", "bool", "int64", "float64", "text", "bytes"]
"""Universal database cell/parameter domain. Engine-native arrays, enums,
unsigned integers, and JSON text sentinels are not baseline ABI values.
"""

DbStatementKind = Literal["execute", "select", "returning"]
"""How the host classifies a statement for result handling."""

DbResultSelection = Literal["discard", "affectedRows", "rows"]
"""Which part of a statement's outcome the caller wants back."""

IsolationReq = Literal["atomicBatch", "nestedSavepoint", "consistentSnapshot"]
"""Host → adapter execute. GuestDatabase stays ExecuteRequest-only."""

ResolvedSqlType = Literal["integer", "real", "text", "blob", "boolean", "null"]
"""Resolved BookclerkSQL column type after type checking."""

IntegerArithKind = Literal["add", "sub", "mul", "abs"]
"""INTEGER `+` `-` `*` `abs` site lowered to overflow -> NULL."""


class ScalarLimits(TypedDict):
    """Guest-advertised caps for `rpc.scalarLimits`; never above the file constants.

    Attributes:
        maxScalarBytes: Largest scalar the guest accepts; at most `maxScalarBytes`.
        maxStreamWindowBytes: Largest `ByteSource.pull` window; at most
            `maxStreamWindowBytes`.
        maxListPage: Largest list page; at most `maxListPage`.
    """

    maxScalarBytes: int
    maxStreamWindowBytes: int
    maxListPage: int


class PluginError(TypedDict):
    """`code` is a snake_case string (`not_found`, `invalid_cursor`, …). Unknown
    codes are forwarded as-is; SDKs surface them as PluginErrorCode.unknown
    while retaining the raw wire code.

    Attributes:
        code: Stable snake_case error code (see `PluginErrorCode`).
        message: Human-readable detail; never contains secrets.
    """

    code: str
    message: str


class ObjectMetadata(TypedDict):
    """Full metadata of one stored object.

    Attributes:
        key: Object key.
        size: Object size in bytes.
        contentType: MIME type; empty when unknown.
        etag: Backend entity tag; empty when unsupported.
        sha256: Raw SHA-256 digest (32 bytes) or empty when unknown.
    """

    key: str
    size: int
    contentType: str
    etag: str
    sha256: bytes


class ObjectInfo(TypedDict):
    """Compact object entry in a list page.

    Attributes:
        key: Object key.
        size: Object size in bytes.
    """

    key: str
    size: int


class ListOptions(TypedDict):
    """Paging options for `Destination.list`.

    Attributes:
        prefix: Only keys starting with this prefix; empty lists everything.
        cursor: Opaque cursor from a previous `ListPage.nextCursor`; empty starts over.
        limit: Requested page size; clamped to `maxListPage`. `0` means guest default.
    """

    prefix: str
    cursor: str
    limit: int


class ListPage(TypedDict):
    """One page of `Destination.list` results.

    Attributes:
        objects: Objects in this page, in backend order.
        nextCursor: Cursor for the next page; empty when exhausted.
    """

    objects: list[ObjectInfo]
    nextCursor: str


class ByteRange(TypedDict):
    """Half-open byte window `[offset, offset + length)`.

    Attributes:
        offset: First byte offset.
        length: Number of bytes; `0` reads to the end.
    """

    offset: int
    length: int


class ReadOptions(TypedDict):
    """Options for `Destination.get`.

    Attributes:
        range: Byte range to read; an all-zero range reads the whole object.
    """

    range: ByteRange


class WriteOptions(TypedDict):
    """Options for `Destination.put`.

    Attributes:
        contentType: MIME type to record; empty when unknown.
        contentLength: Expected body length in bytes; `0` when unknown (chunked).
        sha256: Expected raw SHA-256 digest; empty skips verification.
        commitToken: Destination-side stage-and-publish. Empty means a one-shot put.
        stageOnly: When true, `put` stages remotely and does not publish until `commit`.
    """

    contentType: str
    contentLength: int
    sha256: bytes
    commitToken: str
    stageOnly: bool


class PutResult(TypedDict):
    """Summary of a stored (or committed) object.

    Attributes:
        key: Object key written.
        bytesWritten: Bytes persisted.
        etag: Backend entity tag; empty when unsupported.
        sha256: Raw SHA-256 digest of the stored bytes; empty when not computed.
    """

    key: str
    bytesWritten: int
    etag: str
    sha256: bytes


class CopyResult(TypedDict):
    """Summary of a server-side copy.

    Attributes:
        bytesCopied: Bytes copied.
    """

    bytesCopied: int


class PluginDescribe(TypedDict):
    """Guest identity and negotiation surface returned by `describe()`.

    Attributes:
        apiVersion: ABI version the guest speaks; must equal `apiVersion`.
        id: Stable plugin id (`[a-z][a-z0-9_]{0,63}`).
        kind: Manifest kind (`source`, `integration`, `output`, `database`).
        displayName: Human-readable name for UI lists.
        rpcFeatures: Negotiable feature names the guest supports (see `feature*`
            constants).
        scalarLimits: Guest caps when `rpc.scalarLimits` is advertised.
        supportedRoles: Advertised factories (`destination`, `source`, `worker`,
            `contentSource`, `integration`, `database`). Host still intersects with the
            manifest allowlist.
        metadataJson: Identity extras (brand, cli schema, method names, aliases).
            Versioned JSON escape hatch; not a substitute for typed fields.
    """

    apiVersion: int
    id: str
    kind: str
    displayName: str
    rpcFeatures: list[str]
    scalarLimits: ScalarLimits
    supportedRoles: list[str]
    metadataJson: str


class OidcClientTemplate(TypedDict):
    """Bookclerk-as-IdP relying-party template. Plugins declare callback path and
    client id; the host materializes `oidc_clients` rows and remains the AS.
    `originConfigKey` is a dotted config path (e.g. integrations.audiobookshelf.base_url).

    Attributes:
        clientId: OIDC client id the host materializes.
        displayName: Name shown on consent / admin screens.
        callbackPath: Redirect URI path relative to the integration origin.
        publicClient: When true, the client uses PKCE without a client secret.
        defaultScopes: Scopes granted by default.
        issueRefreshToken: When true, the AS issues refresh tokens to this client.
        originConfigKey: Dotted config path holding the client's origin URL.
    """

    clientId: str
    displayName: str
    callbackPath: str
    publicClient: bool
    defaultScopes: list[str]
    issueRefreshToken: bool
    originConfigKey: str


class OidcClientsOk(TypedDict):
    """Success payload of `BookclerkPlugin.oidcClients`.

    Attributes:
        clients: Client templates; empty when the plugin is not a relying party.
    """

    clients: list[OidcClientTemplate]


class OidcClientsReplyOk(TypedDict):
    """``OidcClientsReply`` member ``ok``.

    Success: OIDC client templates.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: OIDC client templates.
    """

    kind: Literal["ok"]
    value: OidcClientsOk


class OidcClientsReplyErr(TypedDict):
    """``OidcClientsReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


OidcClientsReply = Union[
    OidcClientsReplyOk,
    OidcClientsReplyErr,
]
"""Result union of `BookclerkPlugin.oidcClients`."""


class ExtensibleConfig(TypedDict):
    """Plugin-specific extensible config. Not a substitute for typed ABI fields.

    Attributes:
        schemaVersion: Version of `payload`'s schema, owned by the plugin.
        mediaType: Media type of `payload` (e.g. `application/json`).
        payload: Bounded encoded payload; at most `maxConfigPayloadBytes`.
    """

    schemaVersion: int
    mediaType: str
    payload: bytes


class DestinationContext(TypedDict):
    """Opaque JSON knobs only (migration bridge). Prefer `config` for new fields.
    OS paths, FDs, and sockets are transport-private.

    Attributes:
        json: Legacy opaque JSON knobs (migration bridge).
        config: Granted extensible configuration.
    """

    json: str
    config: ExtensibleConfig


class SourceContext(TypedDict):
    """Granted configuration for `BookclerkPlugin.source`.

    Attributes:
        json: Legacy opaque JSON knobs (migration bridge).
        config: Granted extensible configuration.
    """

    json: str
    config: ExtensibleConfig


class WorkerContext(TypedDict):
    """Granted configuration for `BookclerkPlugin.worker`.

    Attributes:
        jobId: Host job id this handler serves.
        json: Legacy opaque JSON knobs (migration bridge).
        config: Granted extensible configuration.
    """

    jobId: str
    json: str
    config: ExtensibleConfig


class ContentSourceContext(TypedDict):
    """Granted configuration for `BookclerkPlugin.contentSource`.

    Attributes:
        json: Legacy opaque JSON knobs (migration bridge).
        config: Granted extensible configuration.
    """

    json: str
    config: ExtensibleConfig


class IntegrationContext(TypedDict):
    """Granted configuration for `BookclerkPlugin.integration`.

    Attributes:
        json: Legacy opaque JSON knobs (migration bridge).
        config: Granted extensible configuration.
    """

    json: str
    config: ExtensibleConfig


class DatabaseContext(TypedDict):
    """Granted configuration for `BookclerkPlugin.database` (see `DatabaseAdapterConfig`).

    Attributes:
        json: Legacy opaque JSON knobs (migration bridge).
        config: Granted extensible configuration.
    """

    json: str
    config: ExtensibleConfig


class JobInvocation(TypedDict):
    """Durable command envelope (not a domain event). Command payload schema
    version and checkpoint schema versions are independent of the plugin ABI.
    The envelope itself is not persisted as opaque bytes across ABI majors.
    Idempotency keys are scoped to (account, plugin, commandType) until a
    terminal fenced outcome is committed.

    Attributes:
        payloadSchemaVersion: Schema version of `payloadJson`, owned by the command
            type.
        invocationId: Unique id of this invocation attempt.
        commandType: Command type the handler dispatches on.
        payloadJson: Command payload (JSON); at most `maxScalarBytes`.
        idempotencyKey: Caller idempotency key scoped to (account, plugin, commandType).
        attempt: Failure retry counter, starting at 1.
        correlationId: Trace correlation id; empty when none.
        causationId: Id of the event or command that caused this one; empty when none.
        deadlineUnixMs: UTC Unix milliseconds. Host fence/lease is authoritative; this
            hint must not outlive the fence (clock skew across VPS nodes).
        checkpointJson: Checkpoint persisted by a prior `SuspendedOutcome`; empty on
            first run.
        checkpointSchemaVersion: Schema version of `checkpointJson`.
        invocationSequence: Resume ordinal; distinct from failure `attempt`.
        stepId: Optional step identifier for multi-step commands.
    """

    payloadSchemaVersion: int
    invocationId: str
    commandType: str
    payloadJson: str
    idempotencyKey: str
    attempt: int
    correlationId: str
    causationId: str
    deadlineUnixMs: int
    checkpointJson: str
    checkpointSchemaVersion: int
    invocationSequence: int
    stepId: str


class CompletedOutcome(TypedDict):
    """Job finished successfully.

    Attributes:
        message: Short human summary.
        bytesCopied: Bytes produced, when meaningful.
    """

    message: str
    bytesCopied: int


class RetryableOutcome(TypedDict):
    """Job failed transiently; the host reschedules it.

    Attributes:
        message: Short human reason.
        retryAfterUnixMs: Earliest retry time; `0` lets the host choose.
    """

    message: str
    retryAfterUnixMs: int


class RejectedOutcome(TypedDict):
    """Job failed permanently; no retry.

    Attributes:
        message: Short human reason.
    """

    message: str


class CancelledOutcome(TypedDict):
    """Job observed cancellation and stopped.

    Attributes:
        message: Short human note.
    """

    message: str


class SuspendedOutcome(TypedDict):
    """Job released the process and asks to be resumed later.

    Attributes:
        checkpointJson: Bounded checkpoint to replay on resume; at most
            `maxCheckpointBytes`.
        checkpointSchemaVersion: Schema version of `checkpointJson`.
        wakeAtUnixMs: Earliest resume time.
    """

    checkpointJson: str
    checkpointSchemaVersion: int
    wakeAtUnixMs: int


class JobOutcomeCompleted(TypedDict):
    """``JobOutcome`` member ``completed``.

    Finished successfully.

    Attributes:
        kind: Always ``"completed"``.
        value: Finished successfully.
    """

    kind: Literal["completed"]
    value: CompletedOutcome


class JobOutcomeRetryable(TypedDict):
    """``JobOutcome`` member ``retryable``.

    Transient failure; retry later.

    Attributes:
        kind: Always ``"retryable"``.
        value: Transient failure; retry later.
    """

    kind: Literal["retryable"]
    value: RetryableOutcome


class JobOutcomeRejected(TypedDict):
    """``JobOutcome`` member ``rejected``.

    Permanent failure.

    Attributes:
        kind: Always ``"rejected"``.
        value: Permanent failure.
    """

    kind: Literal["rejected"]
    value: RejectedOutcome


class JobOutcomeCancelled(TypedDict):
    """``JobOutcome`` member ``cancelled``.

    Stopped on cancellation.

    Attributes:
        kind: Always ``"cancelled"``.
        value: Stopped on cancellation.
    """

    kind: Literal["cancelled"]
    value: CancelledOutcome


class JobOutcomeSuspended(TypedDict):
    """``JobOutcome`` member ``suspended``.

    Released with a checkpoint.

    Attributes:
        kind: Always ``"suspended"``.
        value: Released with a checkpoint.
    """

    kind: Literal["suspended"]
    value: SuspendedOutcome


JobOutcome = Union[
    JobOutcomeCompleted,
    JobOutcomeRetryable,
    JobOutcomeRejected,
    JobOutcomeCancelled,
    JobOutcomeSuspended,
]
"""Terminal or suspended result of `JobHandler.handle`."""


class DomainEvent(TypedDict):
    """Domain event (not a job). Outbox-produced, at-least-once, idempotent consume.

    Attributes:
        eventId: Unique event id (outbox row identity).
        eventType: Dotted event type (e.g. `library.title.added`).
        schemaVersion: Schema version of `payload`, owned by the event type.
        occurredAtUnixMs: When the producer observed the fact.
        accountId: Account scope; empty for host-wide events.
        correlationId: Trace correlation id; empty when none.
        causationId: Id of the command or event that caused this one; empty when none.
        deduplicationKey: Consumer-side idempotency key; stable across redeliveries.
        deliveryAttempt: Delivery counter, starting at 1.
        payload: Encoded event payload; at most `maxEventPayloadBytes`.
        checkpointJson: Append-only. Resume a prior EventResult.suspended.
        checkpointSchemaVersion: Schema version of `checkpointJson`.
        invocationSequence: Resume ordinal; distinct from `deliveryAttempt`.
        resumePending: True when this delivery resumes a prior suspension.
        source: Append-only. Producer plugin id; empty when unknown.
    """

    eventId: str
    eventType: str
    schemaVersion: int
    occurredAtUnixMs: int
    accountId: str
    correlationId: str
    causationId: str
    deduplicationKey: str
    deliveryAttempt: int
    payload: bytes
    checkpointJson: str
    checkpointSchemaVersion: int
    invocationSequence: int
    resumePending: bool
    source: str


class EventAck(TypedDict):
    """Event handled; the host marks it delivered.

    Attributes:
        dummy: Placeholder; the struct carries no data.
    """

    dummy: None


class EventRetry(TypedDict):
    """Redeliver later.

    Attributes:
        retryAtUnixMs: Earliest redelivery time; `0` lets the host choose.
        reason: Short human reason.
    """

    retryAtUnixMs: int
    reason: str


class EventReject(TypedDict):
    """Event rejected; the host records the reason and stops delivering.

    Attributes:
        reason: Short human reason.
    """

    reason: str


class EventDeadLetter(TypedDict):
    """Event moved to the dead-letter queue for operator review.

    Attributes:
        reason: Short human reason.
    """

    reason: str


class EventSuspended(TypedDict):
    """Append-only. Mirrors job SuspendedOutcome; event handlers persist a
    bounded checkpoint and release the process until wakeAtUnixMs. Optional
    wake-on-matching-event fields (empty = timestamp-only).

    Attributes:
        checkpointJson: Bounded checkpoint to replay on resume; at most
            `maxCheckpointBytes`.
        checkpointSchemaVersion: Schema version of `checkpointJson`.
        wakeAtUnixMs: Earliest resume time.
        wakeOnEventType: Also wake when an event of this type arrives; empty disables.
        wakeOnFilterJson: JSON filter applied to matching wake events; empty matches
            all.
    """

    checkpointJson: str
    checkpointSchemaVersion: int
    wakeAtUnixMs: int
    wakeOnEventType: str
    wakeOnFilterJson: str


class EventResultAck(TypedDict):
    """``EventResult`` member ``ack``.

    Handled.

    Attributes:
        kind: Always ``"ack"``.
        value: Handled.
    """

    kind: Literal["ack"]
    value: EventAck


class EventResultRetry(TypedDict):
    """``EventResult`` member ``retry``.

    Redeliver later.

    Attributes:
        kind: Always ``"retry"``.
        value: Redeliver later.
    """

    kind: Literal["retry"]
    value: EventRetry


class EventResultReject(TypedDict):
    """``EventResult`` member ``reject``.

    Stop delivering.

    Attributes:
        kind: Always ``"reject"``.
        value: Stop delivering.
    """

    kind: Literal["reject"]
    value: EventReject


class EventResultDeadLetter(TypedDict):
    """``EventResult`` member ``deadLetter``.

    Park for operator review.

    Attributes:
        kind: Always ``"deadLetter"``.
        value: Park for operator review.
    """

    kind: Literal["deadLetter"]
    value: EventDeadLetter


class EventResultSuspended(TypedDict):
    """``EventResult`` member ``suspended``.

    Released with a checkpoint.

    Attributes:
        kind: Always ``"suspended"``.
        value: Released with a checkpoint.
    """

    kind: Literal["suspended"]
    value: EventSuspended


EventResult = Union[
    EventResultAck,
    EventResultRetry,
    EventResultReject,
    EventResultDeadLetter,
    EventResultSuspended,
]
"""Outcome of `Integration.onEvent`."""


class HeadOk(TypedDict):
    """Success payload of `Destination.head`.

    Attributes:
        found: Whether the object exists.
        meta: Object metadata; meaningful only when `found`.
    """

    found: bool
    meta: ObjectMetadata


class HeadReplyOk(TypedDict):
    """``HeadReply`` member ``ok``.

    Success: head probe outcome.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: head probe outcome.
    """

    kind: Literal["ok"]
    value: HeadOk


class HeadReplyErr(TypedDict):
    """``HeadReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


HeadReply = Union[
    HeadReplyOk,
    HeadReplyErr,
]
"""Result union of `Destination.head`."""


class ListReplyOk(TypedDict):
    """``ListReply`` member ``ok``.

    Success: one list page.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: one list page.
    """

    kind: Literal["ok"]
    value: ListPage


class ListReplyErr(TypedDict):
    """``ListReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


ListReply = Union[
    ListReplyOk,
    ListReplyErr,
]
"""Result union of `Destination.list`."""


class GetOk(TypedDict):
    """Success payload of `Destination.get`.

    Attributes:
        meta: Object metadata.
        body: Streamed object bytes.
    """

    meta: ObjectMetadata
    body: ByteSource


class GetReplyOk(TypedDict):
    """``GetReply`` member ``ok``.

    Success: object metadata and body stream.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: object metadata and body stream.
    """

    kind: Literal["ok"]
    value: GetOk


class GetReplyErr(TypedDict):
    """``GetReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


GetReply = Union[
    GetReplyOk,
    GetReplyErr,
]
"""Result union of `Destination.get`."""


class PutReplyOk(TypedDict):
    """``PutReply`` member ``ok``.

    Success: stored object summary.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: stored object summary.
    """

    kind: Literal["ok"]
    value: PutResult


class PutReplyErr(TypedDict):
    """``PutReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


PutReply = Union[
    PutReplyOk,
    PutReplyErr,
]
"""Result union of `Destination.put` / `Destination.commit`."""


class CopyReplyOk(TypedDict):
    """``CopyReply`` member ``ok``.

    Success: copy summary.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: copy summary.
    """

    kind: Literal["ok"]
    value: CopyResult


class CopyReplyErr(TypedDict):
    """``CopyReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


CopyReply = Union[
    CopyReplyOk,
    CopyReplyErr,
]
"""Result union of `Destination.copy`."""


class EmptyReplyOk(TypedDict):
    """``EmptyReply`` member ``ok``.

    Success: no payload.

    Attributes:
        kind: Always ``"ok"``.
    """

    kind: Literal["ok"]


class EmptyReplyErr(TypedDict):
    """``EmptyReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


EmptyReply = Union[
    EmptyReplyOk,
    EmptyReplyErr,
]
"""Result union of `methods without a success payload`."""


class PullOk(TypedDict):
    """Success payload of `ByteSource.pull`.

    Attributes:
        chunk: Next bytes; may be shorter than requested.
        done: True when the stream is exhausted after `chunk`.
    """

    chunk: bytes
    done: bool


class PullReplyOk(TypedDict):
    """``PullReply`` member ``ok``.

    Success: one stream window.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: one stream window.
    """

    kind: Literal["ok"]
    value: PullOk


class PullReplyErr(TypedDict):
    """``PullReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


PullReply = Union[
    PullReplyOk,
    PullReplyErr,
]
"""Result union of `ByteSource.pull`."""


class OpenOk(TypedDict):
    """Success payload of `Source.open`.

    Attributes:
        meta: Object metadata.
        body: Streamed object bytes.
    """

    meta: ObjectMetadata
    body: ByteSource


class OpenReplyOk(TypedDict):
    """``OpenReply`` member ``ok``.

    Success: object metadata and body stream.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: object metadata and body stream.
    """

    kind: Literal["ok"]
    value: OpenOk


class OpenReplyErr(TypedDict):
    """``OpenReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


OpenReply = Union[
    OpenReplyOk,
    OpenReplyErr,
]
"""Result union of `Source.open`."""


class DescribeReplyOk(TypedDict):
    """``DescribeReply`` member ``ok``.

    Success: plugin identity.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: plugin identity.
    """

    kind: Literal["ok"]
    value: PluginDescribe


class DescribeReplyErr(TypedDict):
    """``DescribeReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


DescribeReply = Union[
    DescribeReplyOk,
    DescribeReplyErr,
]
"""Result union of `BookclerkPlugin.describe`."""


class DestinationReplyOk(TypedDict):
    """``DestinationReply`` member ``ok``.

    Success: opened `Destination` capability.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: opened `Destination` capability.
    """

    kind: Literal["ok"]
    value: Destination


class DestinationReplyErr(TypedDict):
    """``DestinationReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


DestinationReply = Union[
    DestinationReplyOk,
    DestinationReplyErr,
]
"""Result union of `BookclerkPlugin.destination`."""


class SourceReplyOk(TypedDict):
    """``SourceReply`` member ``ok``.

    Success: opened `Source` capability.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: opened `Source` capability.
    """

    kind: Literal["ok"]
    value: Source


class SourceReplyErr(TypedDict):
    """``SourceReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


SourceReply = Union[
    SourceReplyOk,
    SourceReplyErr,
]
"""Result union of `BookclerkPlugin.source`."""


class WorkerReplyOk(TypedDict):
    """``WorkerReply`` member ``ok``.

    Success: opened `JobHandler` capability.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: opened `JobHandler` capability.
    """

    kind: Literal["ok"]
    value: JobHandler


class WorkerReplyErr(TypedDict):
    """``WorkerReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


WorkerReply = Union[
    WorkerReplyOk,
    WorkerReplyErr,
]
"""Result union of `BookclerkPlugin.worker`."""


class HandleReplyOk(TypedDict):
    """``HandleReply`` member ``ok``.

    Success: job outcome.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: job outcome.
    """

    kind: Literal["ok"]
    value: JobOutcome


class HandleReplyErr(TypedDict):
    """``HandleReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


HandleReply = Union[
    HandleReplyOk,
    HandleReplyErr,
]
"""Result union of `JobHandler.handle`."""


class ContentSourceReplyOk(TypedDict):
    """``ContentSourceReply`` member ``ok``.

    Success: opened `ContentSource` capability.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: opened `ContentSource` capability.
    """

    kind: Literal["ok"]
    value: ContentSource


class ContentSourceReplyErr(TypedDict):
    """``ContentSourceReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


ContentSourceReply = Union[
    ContentSourceReplyOk,
    ContentSourceReplyErr,
]
"""Result union of `BookclerkPlugin.contentSource`."""


class IntegrationReplyOk(TypedDict):
    """``IntegrationReply`` member ``ok``.

    Success: opened `Integration` capability.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: opened `Integration` capability.
    """

    kind: Literal["ok"]
    value: Integration


class IntegrationReplyErr(TypedDict):
    """``IntegrationReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


IntegrationReply = Union[
    IntegrationReplyOk,
    IntegrationReplyErr,
]
"""Result union of `BookclerkPlugin.integration`."""


class DatabaseReplyOk(TypedDict):
    """``DatabaseReply`` member ``ok``.

    Success: opened `Database` capability.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: opened `Database` capability.
    """

    kind: Literal["ok"]
    value: Database


class DatabaseReplyErr(TypedDict):
    """``DatabaseReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


DatabaseReply = Union[
    DatabaseReplyOk,
    DatabaseReplyErr,
]
"""Result union of `BookclerkPlugin.database`."""


class EventResultReplyOk(TypedDict):
    """``EventResultReply`` member ``ok``.

    Success: event handling outcome.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: event handling outcome.
    """

    kind: Literal["ok"]
    value: EventResult


class EventResultReplyErr(TypedDict):
    """``EventResultReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


EventResultReply = Union[
    EventResultReplyOk,
    EventResultReplyErr,
]
"""Result union of `Integration.onEvent`."""


class JsonOk(TypedDict):
    """Migration-bridge JSON result. Frozen methods should prefer typed structs;
    plugin-specific DTOs travel as schemaVersion + mediaType + bounded payload
    via ExtensibleConfig, not as unbounded serde dumps.

    Attributes:
        json: JSON text; at most `maxScalarBytes`.
    """

    json: str


class JsonReplyOk(TypedDict):
    """``JsonReply`` member ``ok``.

    Success: JSON text payload.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: JSON text payload.
    """

    kind: Literal["ok"]
    value: JsonOk


class JsonReplyErr(TypedDict):
    """``JsonReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


JsonReply = Union[
    JsonReplyOk,
    JsonReplyErr,
]
"""Result union of `JSON-bridge methods`."""


class HealthOk(TypedDict):
    """Typed liveness report.

    Attributes:
        ok: True when the guest is healthy enough for traffic.
        detail: Short human status line; empty when none.
    """

    ok: bool
    detail: str


class HealthReplyOk(TypedDict):
    """``HealthReply`` member ``ok``.

    Success: health status.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: health status.
    """

    kind: Literal["ok"]
    value: HealthOk


class HealthReplyErr(TypedDict):
    """``HealthReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


HealthReply = Union[
    HealthReplyOk,
    HealthReplyErr,
]
"""Result union of `health`."""


class AdapterSessionReplyOk(TypedDict):
    """``AdapterSessionReply`` member ``ok``.

    Success: opened `AdapterDatabaseSession` capability.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: opened `AdapterDatabaseSession` capability.
    """

    kind: Literal["ok"]
    value: AdapterDatabaseSession


class AdapterSessionReplyErr(TypedDict):
    """``AdapterSessionReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


AdapterSessionReply = Union[
    AdapterSessionReplyOk,
    AdapterSessionReplyErr,
]
"""Result union of `Database.openSession`."""


class GuestDatabaseReplyOk(TypedDict):
    """``GuestDatabaseReply`` member ``ok``.

    Success: opened `GuestDatabase` capability.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: opened `GuestDatabase` capability.
    """

    kind: Literal["ok"]
    value: GuestDatabase


class GuestDatabaseReplyErr(TypedDict):
    """``GuestDatabaseReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


GuestDatabaseReply = Union[
    GuestDatabaseReplyOk,
    GuestDatabaseReplyErr,
]
"""Result union of `guest database opens`."""


class ByteSource(Protocol):
    """Transferred readable byte stream. The capability *is* the stream; callers
    pull bounded windows. Abort is capability drop / RPC cancel. A failed pull
    MUST set `err` — never a successful empty EOF.
    """

    async def pull(self, max_bytes: int) -> PullReply:
        """Pull the next window of bytes. `done = true` on the final chunk.

        Args:
            max_bytes: Upper bound for this window; at most `maxStreamWindowBytes`.

        Returns:
            ``PullReply``
        """
        ...


class Destination(Protocol):
    """Object store the host writes acquired media into (`output.*` plugins).
    Keys are relative object paths; the plugin owns the physical layout.
    """

    async def head(self, key: str) -> HeadReply:
        """Metadata probe. `found = false` is a success, not `not_found`.

        Args:
            key: Object key to probe.

        Returns:
            ``HeadReply``
        """
        ...

    async def list(self, options: ListOptions) -> ListReply:
        """Page through objects under a prefix; at most `maxListPage` per page.

        Args:
            options: Prefix, cursor, and page size.

        Returns:
            ``ListReply``
        """
        ...

    async def get(self, key: str, options: ReadOptions) -> GetReply:
        """Open an object (or a byte range of it) for reading.

        Args:
            key: Object key to read.
            options: Optional byte range.

        Returns:
            ``GetReply``
        """
        ...

    async def put(self, key: str, body: ByteSource, options: WriteOptions) -> PutReply:
        """Store an object from a transferred byte stream.

        Args:
            key: Object key to write.
            body: Streamed object bytes.
            options: Content type, length, digest, staging.

        Returns:
            ``PutReply``
        """
        ...

    async def copy(self, from_: str, to: str) -> CopyReply:
        """Server-side copy (requires `storage.copy`).

        Args:
            from_: Source object key.
            to: Destination object key.

        Returns:
            ``CopyReply``
        """
        ...

    async def delete(self, key: str) -> EmptyReply:
        """Remove an object; deleting a missing key is a success.

        Args:
            key: Object key to remove.

        Returns:
            ``EmptyReply``
        """
        ...

    async def commit(self, key: str, commit_token: str) -> PutReply:
        """Finalize a destination-side staged object. Staging itself is `put` with
        `stageOnly = true`; bytes must stream into destination-managed temp/multipart
        storage, never a complete local spool on host/adapter/broker/guest.

        Args:
            key: Object key that was staged.
            commit_token: Token from `WriteOptions.commitToken`.

        Returns:
            ``PutReply``
        """
        ...

    async def abort_stage(self, key: str, commit_token: str) -> EmptyReply:
        """Discard a staged object without publishing it.

        Args:
            key: Object key that was staged.
            commit_token: Token from `WriteOptions.commitToken`.

        Returns:
            ``EmptyReply``
        """
        ...


class Source(Protocol):
    """Read-only byte source for job inputs (not a storefront)."""

    async def open(self, key: str) -> OpenReply:
        """Open an object for streaming reads.

        Args:
            key: Object key to open.

        Returns:
            ``OpenReply``
        """
        ...


class ProgressSink(Protocol):
    """Host-side progress reporter handed to job handlers."""

    async def report(self, percent: float, message: str) -> EmptyReply:
        """Report progress; the host coalesces frequent updates.

        Args:
            percent: Completion in `[0, 100]`.
            message: Short human-readable status line.

        Returns:
            ``EmptyReply``
        """
        ...


class Cancellation(Protocol):
    """Transport cancellation. SDKs project this into a locally created AbortSignal
    (AbortSignal is not a serializable Workers RPC value).
    """

    async def poll(self) -> bool:
        """Non-blocking check; `true` once the host has fenced the invocation.

        Returns:
            ``bool``
        """
        ...


class JobHandler(Protocol):
    """Job handler returned by `BookclerkPlugin.worker`; runs one durable command."""

    async def handle(self, invocation: JobInvocation, input: Source, output: Destination, progress: ProgressSink, cancel: Cancellation, database: GuestDatabase, databases: list[NamedDatabase]) -> HandleReply:
        """Run one command invocation to a terminal or suspended outcome.

        Args:
            invocation: Durable command envelope.
            input: Job input objects.
            output: Job output object store.
            progress: Progress reporter.
            cancel: Host cancellation probe.
            database: Append-only. Host-mediated typed SQL session.
            databases: Append-only. Named plugin-owned database bindings (Workers-
                style): each entry is an isolated database provisioned by the active
                adapter, separate from the Bookclerk library and from every other
                plugin. Empty when the manifest declares none.

        Returns:
            ``HandleReply``
        """
        ...


class NamedDatabase(TypedDict):
    """One named plugin-owned database binding delivered on `JobHandler.handle`.

    Attributes:
        name: Binding name from `plugin.toml` `capabilities.bindings.databases`.
        database: Isolated typed SQL session for this binding (plugin-owned schema).
    """

    name: str
    database: GuestDatabase


class ContentSource(Protocol):
    """Storefront content source (not byte Source). JSON params/results are a
    migration bridge for existing storefront DTOs.
    """

    async def login(self, params_json: str) -> JsonReply:
        """Connect an account (password or one-shot OAuth).

        Args:
            params_json: `LoginParams` JSON.

        Returns:
            ``JsonReply``
        """
        ...

    async def scan(self, params_json: str) -> JsonReply:
        """Sync library rows for one or more accounts.

        Args:
            params_json: `ScanParams` JSON.

        Returns:
            ``JsonReply``
        """
        ...

    async def fetch_title(self, params_json: str) -> JsonReply:
        """Download and decrypt one title into `cacheDir`.

        Args:
            params_json: `FetchTitleParams` JSON.

        Returns:
            ``JsonReply``
        """
        ...

    async def list_accounts(self) -> JsonReply:
        """Enumerate accounts the guest knows about.

        Returns:
            ``JsonReply``
        """
        ...

    async def login_start(self, params_json: str) -> JsonReply:
        """Begin an interactive OAuth login; returns a session id.

        Args:
            params_json: `LoginStartParams` JSON.

        Returns:
            ``JsonReply``
        """
        ...

    async def login_complete(self, params_json: str) -> JsonReply:
        """Finish an interactive OAuth login started by `loginStart`.

        Args:
            params_json: `LoginCompleteParams` JSON.

        Returns:
            ``JsonReply``
        """
        ...

    async def search_catalog(self, params_json: str) -> JsonReply:
        """Free-text storefront catalog search.

        Args:
            params_json: `SearchCatalogParams` JSON.

        Returns:
            ``JsonReply``
        """
        ...

    async def expand_candidates(self, params_json: str) -> JsonReply:
        """Related-title expansion from a seed title.

        Args:
            params_json: `ExpandCandidatesParams` JSON.

        Returns:
            ``JsonReply``
        """
        ...

    async def purchase_hint(self, params_json: str) -> JsonReply:
        """Purchase link / price hint for one title.

        Args:
            params_json: `PurchaseHintParams` JSON.

        Returns:
            ``JsonReply``
        """
        ...

    async def list_deals(self, params_json: str) -> JsonReply:
        """Current storefront deals.

        Args:
            params_json: `ListDealsParams` JSON.

        Returns:
            ``JsonReply``
        """
        ...

    async def health(self) -> HealthReply:
        """Liveness / readiness probe.

        Returns:
            ``HealthReply``
        """
        ...

    async def diagnose(self) -> JsonReply:
        """Human-readable diagnostic lines (`DiagnoseResult` JSON).

        Returns:
            ``JsonReply``
        """
        ...

    async def catalog_detail(self, params_json: str) -> JsonReply:
        """Full catalog record for one product.

        Args:
            params_json: `CatalogDetailParams` JSON.

        Returns:
            ``JsonReply``
        """
        ...


class Integration(Protocol):
    """Long-running integration (remote library, listening sync, IdP bridge)."""

    async def health(self) -> HealthReply:
        """Liveness / readiness probe.

        Returns:
            ``HealthReply``
        """
        ...

    async def on_event(self, event: DomainEvent) -> EventResultReply:
        """Deliver one domain event (at-least-once; must be idempotent).

        Args:
            event: Event envelope.

        Returns:
            ``EventResultReply``
        """
        ...

    async def start(self) -> EmptyReply:
        """Start background work after the host has granted bindings.

        Returns:
            ``EmptyReply``
        """
        ...

    async def stop(self) -> EmptyReply:
        """Stop background work; the host may drop the capability afterwards.

        Returns:
            ``EmptyReply``
        """
        ...

    async def diagnose(self) -> JsonReply:
        """Human-readable diagnostic lines (`DiagnoseResult` JSON).

        Returns:
            ``JsonReply``
        """
        ...

    async def scan_library(self, params_json: str) -> EmptyReply:
        """Re-sync the remote library.

        Args:
            params_json: `ScanLibraryParams` JSON.

        Returns:
            ``EmptyReply``
        """
        ...

    async def sync_listening(self) -> JsonReply:
        """Push / pull listening progress.

        Returns:
            ``JsonReply``
        """
        ...

    async def authenticate_user(self, params_json: str) -> JsonReply:
        """Verify remote credentials on behalf of the host.

        Args:
            params_json: `AuthenticateUserParams` JSON.

        Returns:
            ``JsonReply``
        """
        ...

    async def poll_events(self) -> JsonReply:
        """Drain events the remote side produced since the last poll.

        Returns:
            ``JsonReply``
        """
        ...


class PluginMetadata(TypedDict):
    """Identity extras carried as JSON in `describe().metadataJson`: portal auth,
    brand colors, config option discovery, and an embedded CLI schema.

    Attributes:
        apiVersion: ABI version the guest speaks; must equal `apiVersion`.
        id: Stable plugin id matching `plugin.toml` / install directory name.
        kind: Plugin kind: "source", "integration", "output", or "database".
        displayName: Human-readable name for UI lists; omitted when absent.
        capabilities: Declared capability method names the guest implements (e.g.
            "health", "login", "fetchTitle").
        portalAuthMode: Portal Accounts connect mode: "oauth" or "password".
        passwordEnvVar: Optional env var name operators may set for password helpers;
            never required for Accounts UI connect.
        aliases: Alternate ids accepted for config / CLI targeting; omitted when empty.
        sortKey: Optional UI sort weight among peers of the same kind.
        brand: Portal brand colors and icon URL for Accounts / library chrome.
        configOptions: Discoverable config option groups for source UIs.
        cli: Optional embedded CLI schema (same shape as `cliDescribe`).
    """

    apiVersion: int
    id: str
    kind: str
    displayName: NotRequired[str]
    capabilities: NotRequired[list[str]]
    portalAuthMode: NotRequired[str]
    passwordEnvVar: NotRequired[str]
    aliases: NotRequired[list[str]]
    sortKey: NotRequired[int]
    brand: NotRequired[Brand]
    configOptions: NotRequired[list[ConfigOption]]
    cli: NotRequired[CliSchema]


class Brand(TypedDict):
    """Portal brand crossing the RPC boundary. Distinct from `plugin.toml`
    `logo`: `iconUrl` is the live URL or data URI the SPA renders.

    Attributes:
        id: Brand id (often matches the plugin id).
        name: Display name shown next to the brand swatch.
        bg: Background CSS color (hex or named).
        fg: Foreground CSS color for text on `bg`.
        accent: Accent CSS color for highlights / CTAs.
        iconUrl: Icon URL or data URI for the portal.
    """

    id: str
    name: str
    bg: str
    fg: str
    accent: str
    iconUrl: str


class ConfigOption(TypedDict):
    """One discoverable config option group advertised for sources.

    Attributes:
        key: Config key under the plugin's `config.toml` table.
        label: Operator-facing label for the option group.
        values: Allowed selectable values for this key.
    """

    key: str
    label: str
    values: list[ConfigOptionValue]


class ConfigOptionValue(TypedDict):
    """One selectable value under a `ConfigOption`.

    Attributes:
        id: Value written to config when selected.
        label: Operator-facing label for this value.
    """

    id: str
    label: str


class CliSchema(TypedDict):
    """Declared plugin CLI surface (`cliDescribe` / metadata `cli` / `plugin.toml`).

    Attributes:
        commands: Commands exposed as `bookclerk plugins <id> <command> ...`.
    """

    commands: list[CliCommandSpec]


class CliCommandSpec(TypedDict):
    """One plugin CLI command under `CliSchema`.

    Attributes:
        name: Command verb after the plugin id (for example "ping").
        about: Short help text for `--help`; omitted when absent.
        args: Argument / flag specs for this command (default empty).
    """

    name: str
    about: NotRequired[str]
    args: NotRequired[list[CliArgSpec]]


class CliArgSpec(TypedDict):
    """One CLI argument or flag under a `CliCommandSpec`.

    Attributes:
        name: Internal arg name used as the key in `CliInvokeParams.args`.
        long: Long flag without leading dashes (e.g. "message" -> `--message`).
        short: Optional short flag character (e.g. "m" -> `-m`).
        kind: Parsed value kind (default "string").
        required: When true, the host rejects invoke if the arg is missing.
        default: Default string form when the operator omits the arg.
        about: Help text for this arg; omitted when absent.
        positional: When true, the arg is positional rather than a flagged option.
    """

    name: str
    long: NotRequired[str]
    short: NotRequired[str]
    kind: NotRequired[CliArgKind]
    required: NotRequired[bool]
    default: NotRequired[str]
    about: NotRequired[str]
    positional: NotRequired[bool]


class CliInvokeParams(TypedDict):
    """Params JSON for `cliInvoke`.

    Attributes:
        command: Command name matching a `CliCommandSpec.name`.
        args: Named argument values (keys match `CliArgSpec.name`; default `{}`).
    """

    command: str
    args: NotRequired[JsonValue]


class CliInvokeResult(TypedDict):
    """Result JSON for `cliInvoke`.

    Attributes:
        exitCode: Process-style exit code (0 = success).
        stdout: Captured standard output text.
        stderr: Captured standard error text.
        json: Optional structured payload for machine consumers; omitted when absent.
    """

    exitCode: int
    stdout: str
    stderr: str
    json: JsonValue


class DatabaseAdapterConfig(TypedDict):
    """Author-facing database adapter configuration carried in
    `DatabaseContext.config` (mediaType
    `application/vnd.bookclerk.db-adapter-config+json`). This is the generic
    bootstrap mechanism for third-party adapters: the operator's granted
    `[database.<id>]` table plus the scoped writable data dir. First-party
    host-managed adapters receive host-private connect params instead.

    Attributes:
        pluginDataDir: Scoped writable directory for this plugin
            (`.../plugins/<id>/data`).
        config: Granted plugin settings (operator `[database.<id>]` table) as a JSON
            object; `{}` when the operator configured nothing.
        binding: Named plugin database binding this open serves; omitted for the primary
            library open. Adapters advertising `DbCapabilities.pluginDatabases` must
            serve each binding from its own isolated database.
        instanceId: Host-issued opaque instance id for this (owner plugin, binding)
            pair. Collision-resistant and stable across re-opens. Omitted for the
            primary library open. Third-party adapters must key isolated databases on
            this value rather than `binding` alone (two plugins may both declare `DB`).
        provision: Append-only. When false, open an existing binding unit and do not
            provision a missing one (read-only backup capture). Omitted/true on older
            hosts means the adapter may create the unit.
    """

    pluginDataDir: str
    config: NotRequired[JsonValue]
    binding: NotRequired[str]
    instanceId: NotRequired[str]
    provision: NotRequired[bool]


class HealthResult(TypedDict):
    """JSON health payload for guests that report identity alongside liveness.
    Role-level `health` RPCs return the typed `HealthOk` instead.

    Attributes:
        ok: When true, the guest considers itself healthy enough for traffic.
        id: Plugin id echo; omitted when the guest does not duplicate identity.
        enabled: Whether the guest believes it is enabled in config; omitted when
            unknown.
        detail: Short human detail for CLI / UI status lines; omitted when absent.
    """

    ok: bool
    id: str
    enabled: bool
    detail: str


class DiagnoseResult(TypedDict):
    """JSON result of `diagnose`. Each line is printed by
    `bookclerk plugins diagnose` / the control plane.

    Attributes:
        lines: Human-readable probe lines (default empty).
    """

    lines: list[str]


class LoginParams(TypedDict):
    """Params JSON for `ContentSource.login`. Password sources fill
    email/password; OAuth sources use callback / external fields. There is no
    files-dir root or library DB path -- only `pluginDataDir`.

    Attributes:
        pluginDataDir: Scoped writable directory for this plugin only
            (`.../plugins/<id>/data`).
        marketplace: Marketplace / locale for the storefront (default empty -> guest
            default).
        label: Optional operator label stored on the account row.
        email: Account email / username for password logins; omitted for pure OAuth.
        password: Account password for password logins; never logged; omitted for OAuth.
        force: When true, overwrite an existing sealed credential for this account.
        callbackBind: Optional bind address for OAuth callback servers (`host:port`).
            Ignored when `callbackIpc` is set (host owns the TCP listener).
        callbackIpc: Host-owned callback IPC endpoint the guest must connect to. When
            set (with `callbackPublicBase`), the guest must not bind a TCP listener.
        callbackPublicBase: Public base URL for the host TCP listener, e.g.
            `http://127.0.0.1:12345`.
        external: When true, use external / paste-redirect OAuth instead of a local
            callback server.
        responseUrl: Pre-supplied OAuth redirect URL (paste flow); omitted otherwise.
        showQr: Prefer QR output when the guest supports it.
        timeoutSecs: Seconds to wait for OAuth callback capture; guest default when
            omitted.
        extra: Store-specific knobs as a JSON object; guests may ignore unknowns.
    """

    pluginDataDir: str
    marketplace: NotRequired[str]
    label: NotRequired[str]
    email: NotRequired[str]
    password: NotRequired[str]
    force: NotRequired[bool]
    callbackBind: NotRequired[str]
    callbackIpc: NotRequired[str]
    callbackPublicBase: NotRequired[str]
    external: NotRequired[bool]
    responseUrl: NotRequired[str]
    showQr: NotRequired[bool]
    timeoutSecs: NotRequired[int]
    extra: NotRequired[JsonValue]


LoginStartParams = LoginParams
"""Params JSON for `ContentSource.loginStart` -- same shape as `LoginParams`."""


class LoginCompleteParams(TypedDict):
    """Params JSON for `ContentSource.loginComplete`.

    Attributes:
        sessionId: Session id previously returned by `loginStart`.
    """

    sessionId: str


class ScanParams(TypedDict):
    """Params JSON for `ContentSource.scan`. Host injects sealed credentials so
    the plugin does not need a private credential store under `pluginDataDir`.

    Attributes:
        pluginDataDir: Scoped plugin data directory.
        accounts: Account ids to scan; empty means all scan-enabled accounts.
        pageSize: Storefront page size (default 50).
        importEpisodes: When true, import podcast/episode-style rows (default true).
        importPlusTitles: When true, import Plus/catalog entitlement titles (default
            true).
        credentials: Host-loaded credential blobs keyed by account id (JSON object).
    """

    pluginDataDir: str
    accounts: NotRequired[list[str]]
    pageSize: NotRequired[int]
    importEpisodes: NotRequired[bool]
    importPlusTitles: NotRequired[bool]
    credentials: NotRequired[JsonValue]


class FetchTitleParams(TypedDict):
    """Params JSON for `ContentSource.fetchTitle`. Plugin writes media under
    `cacheDir` and returns plain (DRM-free) paths. Host injects credentials;
    guests must not open `library.db` or `master.key`.

    Attributes:
        pluginDataDir: Scoped plugin data directory.
        accountId: Account whose credentials apply.
        titleId: Library / storefront title id to download.
        cacheDir: Absolute path the guest should write media into (jail-granted TMPDIR).
        credentials: Host-loaded credential blob for this account; omitted when
            unavailable.
        sourceConfig: Opaque plugin table from `[sources.<id>]`.
        download: Host acquire/download options (JSON object matching host
            DownloadOptions).
    """

    pluginDataDir: str
    accountId: str
    titleId: str
    cacheDir: str
    credentials: NotRequired[JsonValue]
    sourceConfig: NotRequired[JsonValue]
    download: NotRequired[JsonValue]


class SearchCatalogParams(TypedDict):
    """Params JSON for `ContentSource.searchCatalog`.

    Attributes:
        query: Free-text search query.
        region: Storefront region / marketplace code (default empty -> guest default).
        limit: Maximum hits to return (default 20).
        page: 1-based page for storefronts that page (default 1).
        sort: Sort key: "relevance" / "popularity" / "rating" / "title" / "author".
        field: Optional facet ("author" / "narrator" / "series" / "genre").
        language: Preferred content language (soft-prioritize; e.g. "en").
    """

    query: str
    region: NotRequired[str]
    limit: NotRequired[int]
    page: NotRequired[int]
    sort: NotRequired[str]
    field: NotRequired[str]
    language: NotRequired[str]


class ExpandCandidatesParams(TypedDict):
    """Params JSON for `ContentSource.expandCandidates`. Seed fields identify a
    known title; the guest returns related catalog hits.

    Attributes:
        source: Source plugin id hint when expanding across storefronts.
        productId: Seed storefront product id.
        title: Seed title text.
        authors: Seed authors string.
        narrators: Seed narrators string.
        series: Seed series name.
        seriesAsin: Seed series ASIN when known.
        asin: Seed Amazon ASIN.
        isbn: Seed ISBN.
        region: Storefront region / marketplace code.
        limit: Maximum candidates to return (default 20).
    """

    source: str
    productId: str
    title: str
    authors: str
    narrators: str
    series: str
    seriesAsin: str
    asin: str
    isbn: str
    region: str
    limit: int


class PurchaseHintParams(TypedDict):
    """Params JSON for `ContentSource.purchaseHint`. At least one identity field
    (`productId` / `asin` / `isbn` / title+authors) should be set; guests may
    return `invalid_params` when none are usable.

    Attributes:
        productId: Storefront product id when known.
        title: Title text for fuzzy lookup.
        authors: Authors string for fuzzy lookup.
        asin: Amazon ASIN when known.
        isbn: ISBN when known.
        region: Storefront region / marketplace code.
        withPrice: When true, guests should include live price fields when available.
    """

    productId: str
    title: str
    authors: str
    asin: str
    isbn: str
    region: str
    withPrice: bool


class ListDealsParams(TypedDict):
    """Params JSON for `ContentSource.listDeals`.

    Attributes:
        limit: Optional maximum number of deals to return; guest default when omitted.
    """

    limit: int


class CatalogDetailParams(TypedDict):
    """Params JSON for `ContentSource.catalogDetail`.

    Attributes:
        productId: Store product id (Libro ISBN or ISBN-slug).
        isbn: Optional ISBN when it differs from `productId`.
    """

    productId: str
    isbn: NotRequired[str]


class ScanLibraryParams(TypedDict):
    """Params JSON for `Integration.scanLibrary` (remote library sync).

    Attributes:
        force: When true, force a full rescan even if the guest would otherwise
            incremental-sync.
    """

    force: bool


class AuthenticateUserParams(TypedDict):
    """Params JSON for `Integration.authenticateUser`.

    Attributes:
        username: Integration username / login id.
        password: Integration password; never logged by the host.
    """

    username: str
    password: str


class DbValueNull(TypedDict):
    """``DbValue`` member ``null``.

    SQL NULL with its declared type.

    Attributes:
        kind: Always ``"null"``.
        value: SQL NULL with its declared type.
    """

    kind: Literal["null"]
    value: DbType


class DbValueBoolean(TypedDict):
    """``DbValue`` member ``boolean``.

    Boolean.

    Attributes:
        kind: Always ``"boolean"``.
        value: Boolean.
    """

    kind: Literal["boolean"]
    value: bool


class DbValueInt64(TypedDict):
    """``DbValue`` member ``int64``.

    Signed 64-bit integer.

    Attributes:
        kind: Always ``"int64"``.
        value: Signed 64-bit integer.
    """

    kind: Literal["int64"]
    value: int


class DbValueFloat64(TypedDict):
    """``DbValue`` member ``float64``.

    IEEE-754 double.

    Attributes:
        kind: Always ``"float64"``.
        value: IEEE-754 double.
    """

    kind: Literal["float64"]
    value: float


class DbValueText(TypedDict):
    """``DbValue`` member ``text``.

    UTF-8 text.

    Attributes:
        kind: Always ``"text"``.
        value: UTF-8 text.
    """

    kind: Literal["text"]
    value: str


class DbValueBytes(TypedDict):
    """``DbValue`` member ``bytes``.

    Raw bytes.

    Attributes:
        kind: Always ``"bytes"``.
        value: Raw bytes.
    """

    kind: Literal["bytes"]
    value: bytes


DbValue = Union[
    DbValueNull,
    DbValueBoolean,
    DbValueInt64,
    DbValueFloat64,
    DbValueText,
    DbValueBytes,
]
"""One typed SQL cell or bind parameter."""


class DbColumn(TypedDict):
    """Result column descriptor.

    Attributes:
        name: Column name as projected.
        dbType: Declared or inferred column type.
    """

    name: str
    dbType: DbType


class DbRow(TypedDict):
    """One result row.

    Attributes:
        values: Cells in `columns` order.
    """

    values: list[DbValue]


class DbStatement(TypedDict):
    """One guest statement in an `ExecuteRequest`.

    Attributes:
        sql: BookclerkSQL text; at most `maxScalarBytes`.
        parameters: Positional bind values.
        kind: Statement classification.
        maxRows: Row cap for queries; `0` means adapter default.
        resultSelection: Which outcome parts to return.
    """

    sql: str
    parameters: list[DbValue]
    kind: DbStatementKind
    maxRows: int
    resultSelection: DbResultSelection


class ExecuteRequest(TypedDict):
    """Guest statement batch for `GuestDatabase.execute`; runs atomically.

    Attributes:
        operationId: Caller-chosen idempotency key.
        requestHash: SHA-256 hex of the idempotency-relevant request; empty when
            omitted.
        statements: Statements in execution order; at most `maxStatements`.
        deadlineUnixMs: Deadline hint; `0` means none.
    """

    operationId: str
    requestHash: str
    statements: list[DbStatement]
    deadlineUnixMs: int


class SqlSpan(TypedDict):
    """Byte span in the exact canonical SQL string a proof is bound to.

    Attributes:
        start: Inclusive start byte offset.
        end: Exclusive end byte offset.
    """

    start: int
    end: int


class TextCollateSite(TypedDict):
    """One TEXT expression the adapter must collate bytewise (`COLLATE "C"`).

    Attributes:
        span: Identifier or string-literal span in canonical SQL.
    """

    span: SqlSpan


class IntegerArithSite(TypedDict):
    """One INTEGER arithmetic expression that must not wrap or error on overflow.

    Attributes:
        full: Full expression span (`a + b` or `abs(n)`).
        lhs: Left operand (or `abs` argument).
        rhs: Right operand (`abs` repeats `lhs`).
        kind: Operator.
    """

    full: SqlSpan
    lhs: SqlSpan
    rhs: SqlSpan
    kind: IntegerArithKind


class PhysicalAccess(TypedDict):
    """Physical table/column access used for authorization.

    Attributes:
        table: Physical table name.
        column: Empty = table presence only; "*" = projection wildcard.
    """

    table: str
    column: str


class ResolvedAssignment(TypedDict):
    """Destination assignment `lhs = rhs` (INSERT, UPDATE, ...).

    Attributes:
        table: Target table.
        column: Target column.
        dest: Declared column type.
        source: Resolved type of the assigned expression.
    """

    table: str
    column: str
    dest: ResolvedSqlType
    source: ResolvedSqlType


class NamedSqlType(TypedDict):
    """Column name paired with its resolved type.

    Attributes:
        name: Column name.
        sqlType: Resolved type.
    """

    name: str
    sqlType: ResolvedSqlType


class ColumnReference(TypedDict):
    """`REFERENCES` target of one column.

    Attributes:
        refTable: Referenced table.
        refColumns: Referenced columns.
    """

    refTable: str
    refColumns: list[str]


class OptionalColumnReferenceNone(TypedDict):
    """``OptionalColumnReference`` member ``none``.

    Column has no reference.

    Attributes:
        kind: Always ``"none"``.
    """

    kind: Literal["none"]


class OptionalColumnReferenceSome(TypedDict):
    """``OptionalColumnReference`` member ``some``.

    Column references another table.

    Attributes:
        kind: Always ``"some"``.
        value: Column references another table.
    """

    kind: Literal["some"]
    value: ColumnReference


OptionalColumnReference = Union[
    OptionalColumnReferenceNone,
    OptionalColumnReferenceSome,
]
"""Per-column `REFERENCES` slot in `CreateTableSchema`."""


class ForeignKeyConstraint(TypedDict):
    """Table-level `FOREIGN KEY` constraint.

    Attributes:
        columns: Local columns.
        refTable: Referenced table.
        refColumns: Referenced columns.
    """

    columns: list[str]
    refTable: str
    refColumns: list[str]


class TableConstraintPrimaryKey(TypedDict):
    """``TableConstraint`` member ``primaryKey``.

    `PRIMARY KEY (...)` columns.

    Attributes:
        kind: Always ``"primaryKey"``.
        value: `PRIMARY KEY (...)` columns.
    """

    kind: Literal["primaryKey"]
    value: list[str]


class TableConstraintUnique(TypedDict):
    """``TableConstraint`` member ``unique``.

    `UNIQUE (...)` columns.

    Attributes:
        kind: Always ``"unique"``.
        value: `UNIQUE (...)` columns.
    """

    kind: Literal["unique"]
    value: list[str]


class TableConstraintCheck(TypedDict):
    """``TableConstraint`` member ``check``.

    `CHECK (...)` expression text.

    Attributes:
        kind: Always ``"check"``.
        value: `CHECK (...)` expression text.
    """

    kind: Literal["check"]
    value: str


class TableConstraintForeignKey(TypedDict):
    """``TableConstraint`` member ``foreignKey``.

    `FOREIGN KEY (...) REFERENCES ...`.

    Attributes:
        kind: Always ``"foreignKey"``.
        value: `FOREIGN KEY (...) REFERENCES ...`.
    """

    kind: Literal["foreignKey"]
    value: ForeignKeyConstraint


TableConstraint = Union[
    TableConstraintPrimaryKey,
    TableConstraintUnique,
    TableConstraintCheck,
    TableConstraintForeignKey,
]
"""One table-level constraint."""


class CreateTableSchema(TypedDict):
    """Parsed `CREATE TABLE` (canonical SQL v1). Per-column lists align with `columns`.

    Attributes:
        table: Table name.
        columns: Columns with resolved types, in order.
        identityColumn: `INTEGER PRIMARY KEY AUTOINCREMENT` column; empty when none.
        columnNotNull: Per-column NOT NULL flags.
        columnUnique: Per-column UNIQUE flags.
        columnPrimaryKey: Per-column PRIMARY KEY flags.
        columnDefaults: Per-column DEFAULT expression text; empty when none.
        columnChecks: Per-column CHECK expression text; empty when none.
        columnReferences: Per-column REFERENCES targets.
        tableConstraints: Table-level constraints, in order.
    """

    table: str
    columns: list[NamedSqlType]
    identityColumn: str
    columnNotNull: list[bool]
    columnUnique: list[bool]
    columnPrimaryKey: list[bool]
    columnDefaults: list[str]
    columnChecks: list[str]
    columnReferences: list[OptionalColumnReference]
    tableConstraints: list[TableConstraint]


class SchemaCreate(TypedDict):
    """`CREATE TABLE` action with its durable fingerprint.

    Attributes:
        schema: Parsed table schema.
        fingerprint: Structured schema fingerprint (hex SHA-256).
        noop: True when the catalog already holds this exact fingerprint.
    """

    schema: CreateTableSchema
    fingerprint: str
    noop: bool


class SchemaActionNone(TypedDict):
    """``SchemaAction`` member ``none``.

    Not DDL.

    Attributes:
        kind: Always ``"none"``.
    """

    kind: Literal["none"]


class SchemaActionCreate(TypedDict):
    """``SchemaAction`` member ``create``.

    `CREATE TABLE`.

    Attributes:
        kind: Always ``"create"``.
        value: `CREATE TABLE`.
    """

    kind: Literal["create"]
    value: SchemaCreate


class SchemaActionDrop(TypedDict):
    """``SchemaAction`` member ``drop``.

    `DROP TABLE` of the named table.

    Attributes:
        kind: Always ``"drop"``.
        value: `DROP TABLE` of the named table.
    """

    kind: Literal["drop"]
    value: str


SchemaAction = Union[
    SchemaActionNone,
    SchemaActionCreate,
    SchemaActionDrop,
]
"""CREATE/DROP action recorded on a proof."""


class ResolvedStatement(TypedDict):
    """Host-produced typed proof bound to one exact canonical statement.

    Attributes:
        statementHash: SHA-256 hex of the exact canonical SQL this proof claims.
        outputColumns: SELECT / RETURNING / VALUES output columns in order.
        physicalAccesses: Physical tables/columns referenced (authorization).
        assignments: Mutation destination assignments.
        textCollateSites: TEXT expression spans needing bytewise collation.
        integerArithSites: INTEGER overflow sites (`+` `-` `*` `abs`).
        functions: Function names invoked (folded), for authorization.
        schemaAction: DDL action + fingerprint.
    """

    statementHash: str
    outputColumns: list[NamedSqlType]
    physicalAccesses: list[PhysicalAccess]
    assignments: list[ResolvedAssignment]
    textCollateSites: list[TextCollateSite]
    integerArithSites: list[IntegerArithSite]
    functions: list[str]
    schemaAction: SchemaAction


class AdapterReceipt(TypedDict):
    """Replay receipt the host asks the adapter to persist with the batch.

    Attributes:
        guestLen: Guest statement count inside the receipt wrap (excluding host
            prune/select).
        guestHash: Guest `requestHash` compared on replay.
    """

    guestLen: int
    guestHash: str


class AdapterStatement(TypedDict):
    """One host-lowered statement with its proof.

    Attributes:
        sql: Canonical SQL text.
        parameters: Positional bind values.
        kind: Statement classification.
        maxRows: Row cap for queries; `0` means adapter default.
        resultSelection: Which outcome parts to return.
        proof: Typed proof bound to `sql`.
    """

    sql: str
    parameters: list[DbValue]
    kind: DbStatementKind
    maxRows: int
    resultSelection: DbResultSelection
    proof: ResolvedStatement


class AdapterExecuteRequest(TypedDict):
    """Host -> adapter batch: proven statements plus isolation and receipt.

    Attributes:
        operationId: Host-chosen idempotency key.
        requestHash: SHA-256 hex of the idempotency-relevant request.
        statements: Statements in execution order.
        deadlineUnixMs: Deadline hint; `0` means none.
        isolation: Transaction isolation the adapter must realize.
        receipt: Replay receipt to persist; zero/empty when not required.
    """

    operationId: str
    requestHash: str
    statements: list[AdapterStatement]
    deadlineUnixMs: int
    isolation: IsolationReq
    receipt: AdapterReceipt


class StatementResult(TypedDict):
    """Outcome of one statement.

    Attributes:
        rows: Result rows (empty unless `rows` selected).
        columns: Result column descriptors.
        rowsAffected: Rows changed by a mutation.
    """

    rows: list[DbRow]
    columns: list[DbColumn]
    rowsAffected: int


class DbTiming(TypedDict):
    """Engine timing on `ExecuteReply`.

    Attributes:
        attemptElapsedUs: Monotonic duration of this handler attempt (microseconds).
        dbExecutionUs: Engine-reported SQL/transaction time when available (`0` =
            omitted).
        dbTimingSource: How `dbExecutionUs` was measured.
    """

    attemptElapsedUs: int
    dbExecutionUs: int
    dbTimingSource: str


class ExecuteReply(TypedDict):
    """Success payload of `execute`.

    Attributes:
        operationId: Echo of the request `operationId`.
        statements: Per-statement results in request order.
        timing: Engine timing.
    """

    operationId: str
    statements: list[StatementResult]
    timing: DbTiming


class ExecuteResultReplyOk(TypedDict):
    """``ExecuteResultReply`` member ``ok``.

    Success: statement results.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: statement results.
    """

    kind: Literal["ok"]
    value: ExecuteReply


class ExecuteResultReplyErr(TypedDict):
    """``ExecuteResultReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


ExecuteResultReply = Union[
    ExecuteResultReplyOk,
    ExecuteResultReplyErr,
]
"""Result union of `execute`."""


class DbCapabilities(TypedDict):
    """Semantic SQL-contract advertisement. Diagnostic engine identity is not
    part of the capability plane — see `DbBootstrap`.

    Attributes:
        sqlContractVersion: Bookclerk SQL contract version.
        atomicBatch: Guest can run a bounded statement list as one SQL transaction.
        returning: Guest SQL supports `RETURNING`.
        affectedRows: Guest reports `rowsAffected`.
        schemaMigrations: Guest versions schema with a `bookclerk_schema_migrations`
            table.
        cancellation: Guest honors RPC/session cancellation.
        timing: Guest can fill `DbTiming.dbExecutionUs`. Not a host connect minimum.
        maxBinds: Maximum bound parameters per statement.
        maxStatements: Maximum statements in one atomic batch.
        maxResultRows: Maximum rows a query statement may return.
        maxPayloadBytes: Maximum UTF-8 bytes of SQL plus binds per statement.
        maxResultBytes: Maximum encoded bytes of one statement's result rows.
        maxCellBytes: Maximum UTF-8 / blob bytes of one result cell.
        maxRequestBytes: Maximum encoded bytes of one `ExecuteRequest`.
        maxAtomicResultBytes: Maximum encoded bytes of one `ExecuteReply`.
        pluginDatabases: Append-only. Adapter can open additional isolated sessions for
            plugin-owned database bindings (per-binding file / schema / database).
        maxFunctionArgs: Maximum arguments in one physical function call after adapter
            hiding (nested json_object / min / max / coalesce). `0` is unspecified.
        maxSchemaColumns: Maximum columns in one CREATE TABLE / result row. `0` is
            unspecified.
        maxPatternBytes: Maximum UTF-8 bytes of a BookclerkSQL LIKE pattern value
            (literals and TEXT binds). Adapters that expand LIKE into GLOB must
            advertise a conservative value that still fits the physical pattern cap. `0`
            is unspecified.
        maxLoweredStatementBytes: Maximum UTF-8 bytes of sqlite-family lowered SQL the
            adapter can realize for one statement (INTEGER overflow wraps, LIKE→GLOB,
            NULLIF, INSERT OR IGNORE, query LIMIT wrap, bytes-placeholder expansion).
            `0` is unspecified: the host does not enforce a lowered-size ceiling. First-
            party D1 advertises 100000. Hosts compare a standardized Bookclerk lowering
            upper bound against this number and must not branch on engine identity.
        consistentBackupRead: Adapter can expose one stable logical database state while
            the host reads schema, rows, and identity.
        atomicUnitRestore: Adapter can destructively replace one logical database unit
            so an ordinary restore failure does not leave that unit partially replaced.
    """

    sqlContractVersion: int
    atomicBatch: bool
    returning: bool
    affectedRows: bool
    schemaMigrations: bool
    cancellation: bool
    timing: bool
    maxBinds: int
    maxStatements: int
    maxResultRows: int
    maxPayloadBytes: int
    maxResultBytes: int
    maxCellBytes: int
    maxRequestBytes: int
    maxAtomicResultBytes: int
    pluginDatabases: bool
    maxFunctionArgs: int
    maxSchemaColumns: int
    maxPatternBytes: int
    maxLoweredStatementBytes: int
    consistentBackupRead: bool
    atomicUnitRestore: bool


class DbBootstrapReplyOk(TypedDict):
    """``DbBootstrapReply`` member ``ok``.

    Success: bootstrap metadata.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: bootstrap metadata.
    """

    kind: Literal["ok"]
    value: DbBootstrap


class DbBootstrapReplyErr(TypedDict):
    """``DbBootstrapReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


DbBootstrapReply = Union[
    DbBootstrapReplyOk,
    DbBootstrapReplyErr,
]
"""Result union of `AdapterDatabaseSession.bootstrap`."""


class DbBootstrap(TypedDict):
    """Bootstrap-only diagnostic metadata (not a capability).

    Attributes:
        engine: Diagnostic physical engine name. Hosts must not admit or generate SQL
            from this value. Any string is valid.
    """

    engine: str


class DbCapabilitiesReplyOk(TypedDict):
    """``DbCapabilitiesReply`` member ``ok``.

    Success: capability advertisement.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: capability advertisement.
    """

    kind: Literal["ok"]
    value: DbCapabilities


class DbCapabilitiesReplyErr(TypedDict):
    """``DbCapabilitiesReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


DbCapabilitiesReply = Union[
    DbCapabilitiesReplyOk,
    DbCapabilitiesReplyErr,
]
"""Result union of `AdapterDatabaseSession.capabilities`."""


class Database(Protocol):
    """Database adapter returned by `BookclerkPlugin.database`."""

    async def open_session(self) -> AdapterSessionReply:
        """Open one adapter session (capability negotiation + typed execute).

        Returns:
            ``AdapterSessionReply``
        """
        ...


class IdentityHighWater(TypedDict):
    """Adapter-private identity high-water (sqlite_sequence / bookclerk_identity).
    Column names live in the canonical backup schema, not this catalog.

    Attributes:
        table: Table whose identity column the mark belongs to.
        last: Highest generated or stored value that must not be reused.
    """

    table: str
    last: int


class IdentityExportReplyOk(TypedDict):
    """``IdentityExportReply`` member ``ok``.

    Success: identity high-water rows.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: identity high-water rows.
    """

    kind: Literal["ok"]
    value: list[IdentityHighWater]


class IdentityExportReplyErr(TypedDict):
    """``IdentityExportReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


IdentityExportReply = Union[
    IdentityExportReplyOk,
    IdentityExportReplyErr,
]
"""Result union of `AdapterDatabaseSession.exportIdentity`."""


class UserRelationsReplyOk(TypedDict):
    """``UserRelationsReply`` member ``ok``.

    Success: user relation names.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: user relation names.
    """

    kind: Literal["ok"]
    value: list[str]


class UserRelationsReplyErr(TypedDict):
    """``UserRelationsReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


UserRelationsReply = Union[
    UserRelationsReplyOk,
    UserRelationsReplyErr,
]
"""Result union of `AdapterDatabaseSession.listUserRelations`."""


class AdapterDatabaseSession(Protocol):
    """Host ↔ database adapter plugin. Capability negotiation + typed execute only."""

    async def capabilities(self) -> DbCapabilitiesReply:
        """Semantic SQL-contract advertisement for this session.

        Returns:
            ``DbCapabilitiesReply``
        """
        ...

    async def execute(self, request: AdapterExecuteRequest) -> ExecuteResultReply:
        """Canonical SQL + required structured proofs (not JSON).

        Args:
            request: Proven statements plus isolation and receipt.

        Returns:
            ``ExecuteResultReply``
        """
        ...

    async def close(self) -> EmptyReply:
        """Release the session; further calls fail.

        Returns:
            ``EmptyReply``
        """
        ...

    async def bootstrap(self) -> DbBootstrapReply:
        """Bootstrap-only SeaORM proxy metadata (not part of DbCapabilities).

        Returns:
            ``DbBootstrapReply``
        """
        ...

    async def export_identity(self) -> IdentityExportReply:
        """Snapshot/identity/restore primitives (not a SQL dialect API).

        Returns:
            ``IdentityExportReply``
        """
        ...

    async def import_identity(self, rows: list[IdentityHighWater]) -> EmptyReply:
        """Restore identity high-water marks captured by `exportIdentity`.

        Args:
            rows: High-water rows to apply.

        Returns:
            ``EmptyReply``
        """
        ...

    async def list_user_relations(self) -> UserRelationsReply:
        """Names of user relations present in the logical database unit.

        Returns:
            ``UserRelationsReply``
        """
        ...

    async def prepare_unit_restore(self) -> EmptyReply:
        """Enter restore mode for one logical unit (`atomicUnitRestore`).

        Returns:
            ``EmptyReply``
        """
        ...

    async def drop_user_relations(self, names: list[str]) -> EmptyReply:
        """Drop the named user relations during restore.

        Args:
            names: Relation names from `listUserRelations`.

        Returns:
            ``EmptyReply``
        """
        ...

    async def assert_restore_constraints(self) -> EmptyReply:
        """Verify constraints hold after restore rows were written.

        Returns:
            ``EmptyReply``
        """
        ...


class GuestDatabase(Protocol):
    """Host-granted SQL for job plugin authors. SDK `DatabaseBinding` mirrors the
    Cloudflare Workers D1 surface (`prepare`/`bind`/`run`/`first`/`all`/`raw`,
    `batch`, `exec`) over this typed `execute` transport; wire types stay Cap'n
    `ExecuteRequest`/`ExecuteReply`.
    """

    async def execute(self, request: ExecuteRequest) -> ExecuteResultReply:
        """Run one typed statement batch.

        Args:
            request: Statements, binds, and deadline.

        Returns:
            ``ExecuteResultReply``
        """
        ...

    async def close(self) -> EmptyReply:
        """Release the session; further calls fail.

        Returns:
            ``EmptyReply``
        """
        ...


class PluginMigrationOpSchema(TypedDict):
    """``PluginMigrationOp`` member ``schema``.

    Admitted DDL statement text.

    Attributes:
        kind: Always ``"schema"``.
        value: Admitted DDL statement text.
    """

    kind: Literal["schema"]
    value: str


class PluginMigrationOpData(TypedDict):
    """``PluginMigrationOp`` member ``data``.

    Admitted DML statement text.

    Attributes:
        kind: Always ``"data"``.
        value: Admitted DML statement text.
    """

    kind: Literal["data"]
    value: str


PluginMigrationOp = Union[
    PluginMigrationOpSchema,
    PluginMigrationOpData,
]
"""One already-separated BookclerkSQL operation in a plugin-owned migration.
`schema` is admitted DDL; `data` is admitted DML. There is no native SQL
escape hatch.
"""


class PluginMigration(TypedDict):
    """One plugin-owned migration application. `id` is an opaque plugin-chosen
    stable identity (name, UUID, timestamp-like string, or digits-as-text).
    Bookclerk assigns no order, version, or predecessor meaning to `id`.
    Registration order is the forward sequence.

    Attributes:
        id: Opaque plugin-chosen stable identity.
        operations: Operations in forward order; at most `maxPluginMigrationOps`.
    """

    id: str
    operations: list[PluginMigrationOp]


class PluginMigrationsOk(TypedDict):
    """Success payload of `BookclerkPlugin.databaseMigrations`.

    Attributes:
        migrations: At most `maxListPage` entries; aggregate id+SQL bytes at most
            `maxPluginMigrationRegistrationBytes`; total operations at most
            `maxPluginMigrationTotalOps`.
    """

    migrations: list[PluginMigration]


class PluginMigrationsReplyOk(TypedDict):
    """``PluginMigrationsReply`` member ``ok``.

    Success: ordered migration sequence.

    Attributes:
        kind: Always ``"ok"``.
        value: Success: ordered migration sequence.
    """

    kind: Literal["ok"]
    value: PluginMigrationsOk


class PluginMigrationsReplyErr(TypedDict):
    """``PluginMigrationsReply`` member ``err``.

    Typed failure; `code` is a `PluginErrorCode` wire string.

    Attributes:
        kind: Always ``"err"``.
        value: Typed failure; `code` is a `PluginErrorCode` wire string.
    """

    kind: Literal["err"]
    value: PluginError


PluginMigrationsReply = Union[
    PluginMigrationsReplyOk,
    PluginMigrationsReplyErr,
]
"""Result union of `BookclerkPlugin.databaseMigrations`."""


class BookclerkPlugin(Protocol):
    """Plugin bootstrap capability: the guest's root object."""

    async def describe(self) -> DescribeReply:
        """Identity, ABI version, negotiated features, and advertised roles.

        Returns:
            ``DescribeReply``
        """
        ...

    async def destination(self, context: DestinationContext) -> DestinationReply:
        """Open the object-store destination role.

        Args:
            context: Granted destination configuration.

        Returns:
            ``DestinationReply``
        """
        ...

    async def source(self, context: SourceContext) -> SourceReply:
        """Open the byte-source role.

        Args:
            context: Granted source configuration.

        Returns:
            ``SourceReply``
        """
        ...

    async def worker(self, context: WorkerContext) -> WorkerReply:
        """Open a job handler for one durable command.

        Args:
            context: Job id and granted configuration.

        Returns:
            ``WorkerReply``
        """
        ...

    async def shutdown(self) -> EmptyReply:
        """Flush and release resources before the process exits.

        Returns:
            ``EmptyReply``
        """
        ...

    async def content_source(self, context: ContentSourceContext) -> ContentSourceReply:
        """Open the storefront content-source role.

        Args:
            context: Granted storefront configuration.

        Returns:
            ``ContentSourceReply``
        """
        ...

    async def integration(self, context: IntegrationContext) -> IntegrationReply:
        """Open the integration role.

        Args:
            context: Granted integration configuration.

        Returns:
            ``IntegrationReply``
        """
        ...

    async def database(self, context: DatabaseContext) -> DatabaseReply:
        """Open the database adapter role.

        Args:
            context: Granted adapter configuration.

        Returns:
            ``DatabaseReply``
        """
        ...

    async def cli_describe(self) -> JsonReply:
        """Declared CLI surface (`CliSchema` JSON).

        Returns:
            ``JsonReply``
        """
        ...

    async def cli_invoke(self, params_json: str) -> JsonReply:
        """Run one plugin CLI command (`CliInvokeParams` -> `CliInvokeResult` JSON).

        Args:
            params_json: `CliInvokeParams` JSON.

        Returns:
            ``JsonReply``
        """
        ...

    async def oidc_clients(self) -> OidcClientsReply:
        """Plugin-provided OIDC AS client templates. Empty list when unused.

        Returns:
            ``OidcClientsReply``
        """
        ...

    async def database_migrations(self, binding: str) -> PluginMigrationsReply:
        """Complete ordered plugin-owned migration sequence for one named binding.
        Host calls this at binding initialization, before ordinary execute.
        Empty list means the binding has no plugin-owned migrations.
        Bounded by `maxListPage` / `maxPluginMigrationOps` /
        `maxPluginMigrationTotalOps` / `maxScalarBytes` /
        `maxPluginMigrationRegistrationBytes`.

        Args:
            binding: Binding name from `plugin.toml`.

        Returns:
            ``PluginMigrationsReply``
        """
        ...


__all__ = [
    "JsonValue",
    "JsonObject",
    "PluginErrorCode",
    "CliArgKind",
    "DbType",
    "DbStatementKind",
    "DbResultSelection",
    "IsolationReq",
    "ResolvedSqlType",
    "IntegerArithKind",
    "ScalarLimits",
    "PluginError",
    "ObjectMetadata",
    "ObjectInfo",
    "ListOptions",
    "ListPage",
    "ByteRange",
    "ReadOptions",
    "WriteOptions",
    "PutResult",
    "CopyResult",
    "PluginDescribe",
    "OidcClientTemplate",
    "OidcClientsOk",
    "OidcClientsReplyOk",
    "OidcClientsReplyErr",
    "OidcClientsReply",
    "ExtensibleConfig",
    "DestinationContext",
    "SourceContext",
    "WorkerContext",
    "ContentSourceContext",
    "IntegrationContext",
    "DatabaseContext",
    "JobInvocation",
    "CompletedOutcome",
    "RetryableOutcome",
    "RejectedOutcome",
    "CancelledOutcome",
    "SuspendedOutcome",
    "JobOutcomeCompleted",
    "JobOutcomeRetryable",
    "JobOutcomeRejected",
    "JobOutcomeCancelled",
    "JobOutcomeSuspended",
    "JobOutcome",
    "DomainEvent",
    "EventAck",
    "EventRetry",
    "EventReject",
    "EventDeadLetter",
    "EventSuspended",
    "EventResultAck",
    "EventResultRetry",
    "EventResultReject",
    "EventResultDeadLetter",
    "EventResultSuspended",
    "EventResult",
    "HeadOk",
    "HeadReplyOk",
    "HeadReplyErr",
    "HeadReply",
    "ListReplyOk",
    "ListReplyErr",
    "ListReply",
    "GetOk",
    "GetReplyOk",
    "GetReplyErr",
    "GetReply",
    "PutReplyOk",
    "PutReplyErr",
    "PutReply",
    "CopyReplyOk",
    "CopyReplyErr",
    "CopyReply",
    "EmptyReplyOk",
    "EmptyReplyErr",
    "EmptyReply",
    "PullOk",
    "PullReplyOk",
    "PullReplyErr",
    "PullReply",
    "OpenOk",
    "OpenReplyOk",
    "OpenReplyErr",
    "OpenReply",
    "DescribeReplyOk",
    "DescribeReplyErr",
    "DescribeReply",
    "DestinationReplyOk",
    "DestinationReplyErr",
    "DestinationReply",
    "SourceReplyOk",
    "SourceReplyErr",
    "SourceReply",
    "WorkerReplyOk",
    "WorkerReplyErr",
    "WorkerReply",
    "HandleReplyOk",
    "HandleReplyErr",
    "HandleReply",
    "ContentSourceReplyOk",
    "ContentSourceReplyErr",
    "ContentSourceReply",
    "IntegrationReplyOk",
    "IntegrationReplyErr",
    "IntegrationReply",
    "DatabaseReplyOk",
    "DatabaseReplyErr",
    "DatabaseReply",
    "EventResultReplyOk",
    "EventResultReplyErr",
    "EventResultReply",
    "JsonOk",
    "JsonReplyOk",
    "JsonReplyErr",
    "JsonReply",
    "HealthOk",
    "HealthReplyOk",
    "HealthReplyErr",
    "HealthReply",
    "AdapterSessionReplyOk",
    "AdapterSessionReplyErr",
    "AdapterSessionReply",
    "GuestDatabaseReplyOk",
    "GuestDatabaseReplyErr",
    "GuestDatabaseReply",
    "ByteSource",
    "Destination",
    "Source",
    "ProgressSink",
    "Cancellation",
    "JobHandler",
    "NamedDatabase",
    "ContentSource",
    "Integration",
    "PluginMetadata",
    "Brand",
    "ConfigOption",
    "ConfigOptionValue",
    "CliSchema",
    "CliCommandSpec",
    "CliArgSpec",
    "CliInvokeParams",
    "CliInvokeResult",
    "DatabaseAdapterConfig",
    "HealthResult",
    "DiagnoseResult",
    "LoginParams",
    "LoginStartParams",
    "LoginCompleteParams",
    "ScanParams",
    "FetchTitleParams",
    "SearchCatalogParams",
    "ExpandCandidatesParams",
    "PurchaseHintParams",
    "ListDealsParams",
    "CatalogDetailParams",
    "ScanLibraryParams",
    "AuthenticateUserParams",
    "DbValueNull",
    "DbValueBoolean",
    "DbValueInt64",
    "DbValueFloat64",
    "DbValueText",
    "DbValueBytes",
    "DbValue",
    "DbColumn",
    "DbRow",
    "DbStatement",
    "ExecuteRequest",
    "SqlSpan",
    "TextCollateSite",
    "IntegerArithSite",
    "PhysicalAccess",
    "ResolvedAssignment",
    "NamedSqlType",
    "ColumnReference",
    "OptionalColumnReferenceNone",
    "OptionalColumnReferenceSome",
    "OptionalColumnReference",
    "ForeignKeyConstraint",
    "TableConstraintPrimaryKey",
    "TableConstraintUnique",
    "TableConstraintCheck",
    "TableConstraintForeignKey",
    "TableConstraint",
    "CreateTableSchema",
    "SchemaCreate",
    "SchemaActionNone",
    "SchemaActionCreate",
    "SchemaActionDrop",
    "SchemaAction",
    "ResolvedStatement",
    "AdapterReceipt",
    "AdapterStatement",
    "AdapterExecuteRequest",
    "StatementResult",
    "DbTiming",
    "ExecuteReply",
    "ExecuteResultReplyOk",
    "ExecuteResultReplyErr",
    "ExecuteResultReply",
    "DbCapabilities",
    "DbBootstrapReplyOk",
    "DbBootstrapReplyErr",
    "DbBootstrapReply",
    "DbBootstrap",
    "DbCapabilitiesReplyOk",
    "DbCapabilitiesReplyErr",
    "DbCapabilitiesReply",
    "Database",
    "IdentityHighWater",
    "IdentityExportReplyOk",
    "IdentityExportReplyErr",
    "IdentityExportReply",
    "UserRelationsReplyOk",
    "UserRelationsReplyErr",
    "UserRelationsReply",
    "AdapterDatabaseSession",
    "GuestDatabase",
    "PluginMigrationOpSchema",
    "PluginMigrationOpData",
    "PluginMigrationOp",
    "PluginMigration",
    "PluginMigrationsOk",
    "PluginMigrationsReplyOk",
    "PluginMigrationsReplyErr",
    "PluginMigrationsReply",
    "BookclerkPlugin",
]
