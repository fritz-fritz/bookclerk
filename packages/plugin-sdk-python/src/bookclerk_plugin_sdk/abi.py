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
"""``DbStatementKind`` wire names."""

DbResultSelection = Literal["discard", "affectedRows", "rows"]
"""``DbResultSelection`` wire names."""

IsolationReq = Literal["atomicBatch", "nestedSavepoint", "consistentSnapshot"]
"""Host → adapter execute. GuestDatabase stays ExecuteRequest-only."""

ResolvedSqlType = Literal["integer", "real", "text", "blob", "boolean", "null"]
"""``ResolvedSqlType`` wire names."""

IntegerArithKind = Literal["add", "sub", "mul", "abs"]
"""``IntegerArithKind`` wire names."""


class ScalarLimits(TypedDict):
    """(undocumented)

    Attributes:
        maxScalarBytes: maxScalarBytes
        maxStreamWindowBytes: maxStreamWindowBytes
        maxListPage: maxListPage
    """

    maxScalarBytes: int
    maxStreamWindowBytes: int
    maxListPage: int


class PluginError(TypedDict):
    """`code` is a snake_case string (`not_found`, `invalid_cursor`, …). Unknown
    codes are forwarded as-is; SDKs surface them as PluginErrorCode.unknown
    while retaining the raw wire code.

    Attributes:
        code: code
        message: message
    """

    code: str
    message: str


class ObjectMetadata(TypedDict):
    """(undocumented)

    Attributes:
        key: key
        size: size
        contentType: contentType
        etag: etag
        sha256: sha256
    """

    key: str
    size: int
    contentType: str
    etag: str
    sha256: bytes


class ObjectInfo(TypedDict):
    """(undocumented)

    Attributes:
        key: key
        size: size
    """

    key: str
    size: int


class ListOptions(TypedDict):
    """(undocumented)

    Attributes:
        prefix: prefix
        cursor: cursor
        limit: limit
    """

    prefix: str
    cursor: str
    limit: int


class ListPage(TypedDict):
    """(undocumented)

    Attributes:
        objects: objects
        nextCursor: nextCursor
    """

    objects: list[ObjectInfo]
    nextCursor: str


class ByteRange(TypedDict):
    """(undocumented)

    Attributes:
        offset: offset
        length: length
    """

    offset: int
    length: int


class ReadOptions(TypedDict):
    """(undocumented)

    Attributes:
        range: range
    """

    range: ByteRange


class WriteOptions(TypedDict):
    """(undocumented)

    Attributes:
        contentType: contentType
        contentLength: contentLength
        sha256: sha256
        commitToken: Destination-side stage-and-publish. Empty means a one-shot put.
        stageOnly: When true, `put` stages remotely and does not publish until `commit`.
    """

    contentType: str
    contentLength: int
    sha256: bytes
    commitToken: str
    stageOnly: bool


class PutResult(TypedDict):
    """(undocumented)

    Attributes:
        key: key
        bytesWritten: bytesWritten
        etag: etag
        sha256: sha256
    """

    key: str
    bytesWritten: int
    etag: str
    sha256: bytes


class CopyResult(TypedDict):
    """(undocumented)

    Attributes:
        bytesCopied: bytesCopied
    """

    bytesCopied: int


class PluginDescribe(TypedDict):
    """(undocumented)

    Attributes:
        apiVersion: apiVersion
        id: id
        kind: kind
        displayName: displayName
        rpcFeatures: rpcFeatures
        scalarLimits: scalarLimits
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
        clientId: clientId
        displayName: displayName
        callbackPath: callbackPath
        publicClient: publicClient
        defaultScopes: defaultScopes
        issueRefreshToken: issueRefreshToken
        originConfigKey: originConfigKey
    """

    clientId: str
    displayName: str
    callbackPath: str
    publicClient: bool
    defaultScopes: list[str]
    issueRefreshToken: bool
    originConfigKey: str


class OidcClientsOk(TypedDict):
    """(undocumented)

    Attributes:
        clients: clients
    """

    clients: list[OidcClientTemplate]


class OidcClientsReplyOk(TypedDict):
    """``OidcClientsReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``OidcClientsOk`` payload.
    """

    kind: Literal["ok"]
    value: OidcClientsOk


class OidcClientsReplyErr(TypedDict):
    """``OidcClientsReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


OidcClientsReply = Union[
    OidcClientsReplyOk,
    OidcClientsReplyErr,
]
"""``OidcClientsReply`` union."""


class ExtensibleConfig(TypedDict):
    """Plugin-specific extensible config. Not a substitute for typed ABI fields.

    Attributes:
        schemaVersion: schemaVersion
        mediaType: mediaType
        payload: payload
    """

    schemaVersion: int
    mediaType: str
    payload: bytes


class DestinationContext(TypedDict):
    """Opaque JSON knobs only (migration bridge). Prefer `config` for new fields.
    OS paths, FDs, and sockets are transport-private.

    Attributes:
        json: json
        config: config
    """

    json: str
    config: ExtensibleConfig


class SourceContext(TypedDict):
    """(undocumented)

    Attributes:
        json: json
        config: config
    """

    json: str
    config: ExtensibleConfig


class WorkerContext(TypedDict):
    """(undocumented)

    Attributes:
        jobId: jobId
        json: json
        config: config
    """

    jobId: str
    json: str
    config: ExtensibleConfig


class ContentSourceContext(TypedDict):
    """(undocumented)

    Attributes:
        json: json
        config: config
    """

    json: str
    config: ExtensibleConfig


class IntegrationContext(TypedDict):
    """(undocumented)

    Attributes:
        json: json
        config: config
    """

    json: str
    config: ExtensibleConfig


class DatabaseContext(TypedDict):
    """(undocumented)

    Attributes:
        json: json
        config: config
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
        payloadSchemaVersion: payloadSchemaVersion
        invocationId: invocationId
        commandType: commandType
        payloadJson: payloadJson
        idempotencyKey: idempotencyKey
        attempt: attempt
        correlationId: correlationId
        causationId: causationId
        deadlineUnixMs: UTC Unix milliseconds. Host fence/lease is authoritative; this
            hint must not outlive the fence (clock skew across VPS nodes).
        checkpointJson: checkpointJson
        checkpointSchemaVersion: checkpointSchemaVersion
        invocationSequence: Resume ordinal; distinct from failure `attempt`.
        stepId: stepId
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
    """(undocumented)

    Attributes:
        message: message
        bytesCopied: bytesCopied
    """

    message: str
    bytesCopied: int


class RetryableOutcome(TypedDict):
    """(undocumented)

    Attributes:
        message: message
        retryAfterUnixMs: retryAfterUnixMs
    """

    message: str
    retryAfterUnixMs: int


class RejectedOutcome(TypedDict):
    """(undocumented)

    Attributes:
        message: message
    """

    message: str


class CancelledOutcome(TypedDict):
    """(undocumented)

    Attributes:
        message: message
    """

    message: str


class SuspendedOutcome(TypedDict):
    """(undocumented)

    Attributes:
        checkpointJson: checkpointJson
        checkpointSchemaVersion: checkpointSchemaVersion
        wakeAtUnixMs: wakeAtUnixMs
    """

    checkpointJson: str
    checkpointSchemaVersion: int
    wakeAtUnixMs: int


class JobOutcomeCompleted(TypedDict):
    """``JobOutcome`` member ``completed``.

    Attributes:
        kind: Always ``"completed"``.
        value: ``CompletedOutcome`` payload.
    """

    kind: Literal["completed"]
    value: CompletedOutcome


class JobOutcomeRetryable(TypedDict):
    """``JobOutcome`` member ``retryable``.

    Attributes:
        kind: Always ``"retryable"``.
        value: ``RetryableOutcome`` payload.
    """

    kind: Literal["retryable"]
    value: RetryableOutcome


class JobOutcomeRejected(TypedDict):
    """``JobOutcome`` member ``rejected``.

    Attributes:
        kind: Always ``"rejected"``.
        value: ``RejectedOutcome`` payload.
    """

    kind: Literal["rejected"]
    value: RejectedOutcome


class JobOutcomeCancelled(TypedDict):
    """``JobOutcome`` member ``cancelled``.

    Attributes:
        kind: Always ``"cancelled"``.
        value: ``CancelledOutcome`` payload.
    """

    kind: Literal["cancelled"]
    value: CancelledOutcome


class JobOutcomeSuspended(TypedDict):
    """``JobOutcome`` member ``suspended``.

    Attributes:
        kind: Always ``"suspended"``.
        value: ``SuspendedOutcome`` payload.
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
"""``JobOutcome`` union."""


class DomainEvent(TypedDict):
    """Domain event (not a job). Outbox-produced, at-least-once, idempotent consume.

    Attributes:
        eventId: eventId
        eventType: eventType
        schemaVersion: schemaVersion
        occurredAtUnixMs: occurredAtUnixMs
        accountId: accountId
        correlationId: correlationId
        causationId: causationId
        deduplicationKey: deduplicationKey
        deliveryAttempt: deliveryAttempt
        payload: payload
        checkpointJson: Append-only. Resume a prior EventResult.suspended.
        checkpointSchemaVersion: checkpointSchemaVersion
        invocationSequence: invocationSequence
        resumePending: resumePending
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
    """(undocumented)

    Attributes:
        dummy: dummy
    """

    dummy: None


class EventRetry(TypedDict):
    """(undocumented)

    Attributes:
        retryAtUnixMs: retryAtUnixMs
        reason: reason
    """

    retryAtUnixMs: int
    reason: str


class EventReject(TypedDict):
    """(undocumented)

    Attributes:
        reason: reason
    """

    reason: str


class EventDeadLetter(TypedDict):
    """(undocumented)

    Attributes:
        reason: reason
    """

    reason: str


class EventSuspended(TypedDict):
    """Append-only. Mirrors job SuspendedOutcome; event handlers persist a
    bounded checkpoint and release the process until wakeAtUnixMs. Optional
    wake-on-matching-event fields (empty = timestamp-only).

    Attributes:
        checkpointJson: checkpointJson
        checkpointSchemaVersion: checkpointSchemaVersion
        wakeAtUnixMs: wakeAtUnixMs
        wakeOnEventType: wakeOnEventType
        wakeOnFilterJson: wakeOnFilterJson
    """

    checkpointJson: str
    checkpointSchemaVersion: int
    wakeAtUnixMs: int
    wakeOnEventType: str
    wakeOnFilterJson: str


class EventResultAck(TypedDict):
    """``EventResult`` member ``ack``.

    Attributes:
        kind: Always ``"ack"``.
        value: ``EventAck`` payload.
    """

    kind: Literal["ack"]
    value: EventAck


class EventResultRetry(TypedDict):
    """``EventResult`` member ``retry``.

    Attributes:
        kind: Always ``"retry"``.
        value: ``EventRetry`` payload.
    """

    kind: Literal["retry"]
    value: EventRetry


class EventResultReject(TypedDict):
    """``EventResult`` member ``reject``.

    Attributes:
        kind: Always ``"reject"``.
        value: ``EventReject`` payload.
    """

    kind: Literal["reject"]
    value: EventReject


class EventResultDeadLetter(TypedDict):
    """``EventResult`` member ``deadLetter``.

    Attributes:
        kind: Always ``"deadLetter"``.
        value: ``EventDeadLetter`` payload.
    """

    kind: Literal["deadLetter"]
    value: EventDeadLetter


class EventResultSuspended(TypedDict):
    """``EventResult`` member ``suspended``.

    Attributes:
        kind: Always ``"suspended"``.
        value: ``EventSuspended`` payload.
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
"""``EventResult`` union."""


class HeadOk(TypedDict):
    """(undocumented)

    Attributes:
        found: found
        meta: meta
    """

    found: bool
    meta: ObjectMetadata


class HeadReplyOk(TypedDict):
    """``HeadReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``HeadOk`` payload.
    """

    kind: Literal["ok"]
    value: HeadOk


class HeadReplyErr(TypedDict):
    """``HeadReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


HeadReply = Union[
    HeadReplyOk,
    HeadReplyErr,
]
"""``HeadReply`` union."""


class ListReplyOk(TypedDict):
    """``ListReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``ListPage`` payload.
    """

    kind: Literal["ok"]
    value: ListPage


class ListReplyErr(TypedDict):
    """``ListReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


ListReply = Union[
    ListReplyOk,
    ListReplyErr,
]
"""``ListReply`` union."""


class GetOk(TypedDict):
    """(undocumented)

    Attributes:
        meta: meta
        body: body
    """

    meta: ObjectMetadata
    body: ByteSource


class GetReplyOk(TypedDict):
    """``GetReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``GetOk`` payload.
    """

    kind: Literal["ok"]
    value: GetOk


class GetReplyErr(TypedDict):
    """``GetReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


GetReply = Union[
    GetReplyOk,
    GetReplyErr,
]
"""``GetReply`` union."""


class PutReplyOk(TypedDict):
    """``PutReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``PutResult`` payload.
    """

    kind: Literal["ok"]
    value: PutResult


class PutReplyErr(TypedDict):
    """``PutReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


PutReply = Union[
    PutReplyOk,
    PutReplyErr,
]
"""``PutReply`` union."""


class CopyReplyOk(TypedDict):
    """``CopyReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``CopyResult`` payload.
    """

    kind: Literal["ok"]
    value: CopyResult


class CopyReplyErr(TypedDict):
    """``CopyReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


CopyReply = Union[
    CopyReplyOk,
    CopyReplyErr,
]
"""``CopyReply`` union."""


class EmptyReplyOk(TypedDict):
    """``EmptyReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
    """

    kind: Literal["ok"]


class EmptyReplyErr(TypedDict):
    """``EmptyReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


EmptyReply = Union[
    EmptyReplyOk,
    EmptyReplyErr,
]
"""``EmptyReply`` union."""


class PullOk(TypedDict):
    """(undocumented)

    Attributes:
        chunk: chunk
        done: done
    """

    chunk: bytes
    done: bool


class PullReplyOk(TypedDict):
    """``PullReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``PullOk`` payload.
    """

    kind: Literal["ok"]
    value: PullOk


class PullReplyErr(TypedDict):
    """``PullReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


PullReply = Union[
    PullReplyOk,
    PullReplyErr,
]
"""``PullReply`` union."""


class OpenOk(TypedDict):
    """(undocumented)

    Attributes:
        meta: meta
        body: body
    """

    meta: ObjectMetadata
    body: ByteSource


class OpenReplyOk(TypedDict):
    """``OpenReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``OpenOk`` payload.
    """

    kind: Literal["ok"]
    value: OpenOk


class OpenReplyErr(TypedDict):
    """``OpenReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


OpenReply = Union[
    OpenReplyOk,
    OpenReplyErr,
]
"""``OpenReply`` union."""


class DescribeReplyOk(TypedDict):
    """``DescribeReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``PluginDescribe`` payload.
    """

    kind: Literal["ok"]
    value: PluginDescribe


class DescribeReplyErr(TypedDict):
    """``DescribeReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


DescribeReply = Union[
    DescribeReplyOk,
    DescribeReplyErr,
]
"""``DescribeReply`` union."""


class DestinationReplyOk(TypedDict):
    """``DestinationReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``Destination`` payload.
    """

    kind: Literal["ok"]
    value: Destination


class DestinationReplyErr(TypedDict):
    """``DestinationReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


DestinationReply = Union[
    DestinationReplyOk,
    DestinationReplyErr,
]
"""``DestinationReply`` union."""


class SourceReplyOk(TypedDict):
    """``SourceReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``Source`` payload.
    """

    kind: Literal["ok"]
    value: Source


class SourceReplyErr(TypedDict):
    """``SourceReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


SourceReply = Union[
    SourceReplyOk,
    SourceReplyErr,
]
"""``SourceReply`` union."""


class WorkerReplyOk(TypedDict):
    """``WorkerReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``JobHandler`` payload.
    """

    kind: Literal["ok"]
    value: JobHandler


class WorkerReplyErr(TypedDict):
    """``WorkerReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


WorkerReply = Union[
    WorkerReplyOk,
    WorkerReplyErr,
]
"""``WorkerReply`` union."""


class HandleReplyOk(TypedDict):
    """``HandleReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``JobOutcome`` payload.
    """

    kind: Literal["ok"]
    value: JobOutcome


class HandleReplyErr(TypedDict):
    """``HandleReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


HandleReply = Union[
    HandleReplyOk,
    HandleReplyErr,
]
"""``HandleReply`` union."""


class ContentSourceReplyOk(TypedDict):
    """``ContentSourceReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``ContentSource`` payload.
    """

    kind: Literal["ok"]
    value: ContentSource


class ContentSourceReplyErr(TypedDict):
    """``ContentSourceReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


ContentSourceReply = Union[
    ContentSourceReplyOk,
    ContentSourceReplyErr,
]
"""``ContentSourceReply`` union."""


class IntegrationReplyOk(TypedDict):
    """``IntegrationReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``Integration`` payload.
    """

    kind: Literal["ok"]
    value: Integration


class IntegrationReplyErr(TypedDict):
    """``IntegrationReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


IntegrationReply = Union[
    IntegrationReplyOk,
    IntegrationReplyErr,
]
"""``IntegrationReply`` union."""


class DatabaseReplyOk(TypedDict):
    """``DatabaseReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``Database`` payload.
    """

    kind: Literal["ok"]
    value: Database


class DatabaseReplyErr(TypedDict):
    """``DatabaseReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


DatabaseReply = Union[
    DatabaseReplyOk,
    DatabaseReplyErr,
]
"""``DatabaseReply`` union."""


class EventResultReplyOk(TypedDict):
    """``EventResultReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``EventResult`` payload.
    """

    kind: Literal["ok"]
    value: EventResult


class EventResultReplyErr(TypedDict):
    """``EventResultReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


EventResultReply = Union[
    EventResultReplyOk,
    EventResultReplyErr,
]
"""``EventResultReply`` union."""


class JsonOk(TypedDict):
    """Migration-bridge JSON result. Frozen methods should prefer typed structs;
    plugin-specific DTOs travel as schemaVersion + mediaType + bounded payload
    via ExtensibleConfig, not as unbounded serde dumps.

    Attributes:
        json: json
    """

    json: str


class JsonReplyOk(TypedDict):
    """``JsonReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``JsonOk`` payload.
    """

    kind: Literal["ok"]
    value: JsonOk


class JsonReplyErr(TypedDict):
    """``JsonReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


JsonReply = Union[
    JsonReplyOk,
    JsonReplyErr,
]
"""``JsonReply`` union."""


class HealthOk(TypedDict):
    """(undocumented)

    Attributes:
        ok: ok
        detail: detail
    """

    ok: bool
    detail: str


class HealthReplyOk(TypedDict):
    """``HealthReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``HealthOk`` payload.
    """

    kind: Literal["ok"]
    value: HealthOk


class HealthReplyErr(TypedDict):
    """``HealthReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


HealthReply = Union[
    HealthReplyOk,
    HealthReplyErr,
]
"""``HealthReply`` union."""


class AdapterSessionReplyOk(TypedDict):
    """``AdapterSessionReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``AdapterDatabaseSession`` payload.
    """

    kind: Literal["ok"]
    value: AdapterDatabaseSession


class AdapterSessionReplyErr(TypedDict):
    """``AdapterSessionReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


AdapterSessionReply = Union[
    AdapterSessionReplyOk,
    AdapterSessionReplyErr,
]
"""``AdapterSessionReply`` union."""


class GuestDatabaseReplyOk(TypedDict):
    """``GuestDatabaseReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``GuestDatabase`` payload.
    """

    kind: Literal["ok"]
    value: GuestDatabase


class GuestDatabaseReplyErr(TypedDict):
    """``GuestDatabaseReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


GuestDatabaseReply = Union[
    GuestDatabaseReplyOk,
    GuestDatabaseReplyErr,
]
"""``GuestDatabaseReply`` union."""


class ByteSource(Protocol):
    """Transferred readable byte stream. The capability *is* the stream; callers
    pull bounded windows. Abort is capability drop / RPC cancel. A failed pull
    MUST set `err` — never a successful empty EOF.
    """

    async def pull(self, max_bytes: int) -> PullReply:
        """``ByteSource.pull``.

        Args:
            max_bytes: max_bytes

        Returns:
            result
        """
        ...


class Destination(Protocol):
    """``Destination`` capability."""

    async def head(self, key: str) -> HeadReply:
        """``Destination.head``.

        Args:
            key: key

        Returns:
            result
        """
        ...

    async def list(self, options: ListOptions) -> ListReply:
        """``Destination.list``.

        Args:
            options: options

        Returns:
            result
        """
        ...

    async def get(self, key: str, options: ReadOptions) -> GetReply:
        """``Destination.get``.

        Args:
            key: key
            options: options

        Returns:
            result
        """
        ...

    async def put(self, key: str, body: ByteSource, options: WriteOptions) -> PutReply:
        """``Destination.put``.

        Args:
            key: key
            body: body
            options: options

        Returns:
            result
        """
        ...

    async def copy(self, from_: str, to: str) -> CopyReply:
        """``Destination.copy``.

        Args:
            from_: from_
            to: to

        Returns:
            result
        """
        ...

    async def delete(self, key: str) -> EmptyReply:
        """``Destination.delete``.

        Args:
            key: key

        Returns:
            result
        """
        ...

    async def commit(self, key: str, commit_token: str) -> PutReply:
        """Finalize a destination-side staged object. Staging itself is `put` with
        `stageOnly = true`; bytes must stream into destination-managed temp/multipart
        storage, never a complete local spool on host/adapter/broker/guest.

        Args:
            key: key
            commit_token: commit_token

        Returns:
            result
        """
        ...

    async def abort_stage(self, key: str, commit_token: str) -> EmptyReply:
        """``Destination.abortStage``.

        Args:
            key: key
            commit_token: commit_token

        Returns:
            result
        """
        ...


class Source(Protocol):
    """``Source`` capability."""

    async def open(self, key: str) -> OpenReply:
        """``Source.open``.

        Args:
            key: key

        Returns:
            result
        """
        ...


class ProgressSink(Protocol):
    """``ProgressSink`` capability."""

    async def report(self, percent: float, message: str) -> EmptyReply:
        """``ProgressSink.report``.

        Args:
            percent: percent
            message: message

        Returns:
            result
        """
        ...


class Cancellation(Protocol):
    """Transport cancellation. SDKs project this into a locally created AbortSignal
    (AbortSignal is not a serializable Workers RPC value).
    """

    async def poll(self) -> bool:
        """``Cancellation.poll``.

        Returns:
            cancelled
        """
        ...


class JobHandler(Protocol):
    """``JobHandler`` capability."""

    async def handle(self, invocation: JobInvocation, input: Source, output: Destination, progress: ProgressSink, cancel: Cancellation, database: GuestDatabase, databases: list[NamedDatabase]) -> HandleReply:
        """``JobHandler.handle``.

        Args:
            invocation: invocation
            input: input
            output: output
            progress: progress
            cancel: cancel
            database: Append-only. Host-mediated typed SQL session.
            databases: Append-only. Named plugin-owned database bindings (Workers-
                style): each entry is an isolated database provisioned by the active
                adapter, separate from the Bookclerk library and from every other
                plugin. Empty when the manifest declares none.

        Returns:
            result
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
        """``ContentSource.login``.

        Args:
            params_json: params_json

        Returns:
            result
        """
        ...

    async def scan(self, params_json: str) -> JsonReply:
        """``ContentSource.scan``.

        Args:
            params_json: params_json

        Returns:
            result
        """
        ...

    async def fetch_title(self, params_json: str) -> JsonReply:
        """``ContentSource.fetchTitle``.

        Args:
            params_json: params_json

        Returns:
            result
        """
        ...

    async def list_accounts(self) -> JsonReply:
        """``ContentSource.listAccounts``.

        Returns:
            result
        """
        ...

    async def login_start(self, params_json: str) -> JsonReply:
        """``ContentSource.loginStart``.

        Args:
            params_json: params_json

        Returns:
            result
        """
        ...

    async def login_complete(self, params_json: str) -> JsonReply:
        """``ContentSource.loginComplete``.

        Args:
            params_json: params_json

        Returns:
            result
        """
        ...

    async def search_catalog(self, params_json: str) -> JsonReply:
        """``ContentSource.searchCatalog``.

        Args:
            params_json: params_json

        Returns:
            result
        """
        ...

    async def expand_candidates(self, params_json: str) -> JsonReply:
        """``ContentSource.expandCandidates``.

        Args:
            params_json: params_json

        Returns:
            result
        """
        ...

    async def purchase_hint(self, params_json: str) -> JsonReply:
        """``ContentSource.purchaseHint``.

        Args:
            params_json: params_json

        Returns:
            result
        """
        ...

    async def list_deals(self, params_json: str) -> JsonReply:
        """``ContentSource.listDeals``.

        Args:
            params_json: params_json

        Returns:
            result
        """
        ...

    async def health(self) -> HealthReply:
        """``ContentSource.health``.

        Returns:
            result
        """
        ...

    async def diagnose(self) -> JsonReply:
        """``ContentSource.diagnose``.

        Returns:
            result
        """
        ...

    async def catalog_detail(self, params_json: str) -> JsonReply:
        """``ContentSource.catalogDetail``.

        Args:
            params_json: params_json

        Returns:
            result
        """
        ...


class Integration(Protocol):
    """``Integration`` capability."""

    async def health(self) -> HealthReply:
        """``Integration.health``.

        Returns:
            result
        """
        ...

    async def on_event(self, event: DomainEvent) -> EventResultReply:
        """``Integration.onEvent``.

        Args:
            event: event

        Returns:
            result
        """
        ...

    async def start(self) -> EmptyReply:
        """``Integration.start``.

        Returns:
            result
        """
        ...

    async def stop(self) -> EmptyReply:
        """``Integration.stop``.

        Returns:
            result
        """
        ...

    async def diagnose(self) -> JsonReply:
        """``Integration.diagnose``.

        Returns:
            result
        """
        ...

    async def scan_library(self, params_json: str) -> EmptyReply:
        """``Integration.scanLibrary``.

        Args:
            params_json: params_json

        Returns:
            result
        """
        ...

    async def sync_listening(self) -> JsonReply:
        """``Integration.syncListening``.

        Returns:
            result
        """
        ...

    async def authenticate_user(self, params_json: str) -> JsonReply:
        """``Integration.authenticateUser``.

        Args:
            params_json: params_json

        Returns:
            result
        """
        ...

    async def poll_events(self) -> JsonReply:
        """``Integration.pollEvents``.

        Returns:
            result
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

    Attributes:
        kind: Always ``"null"``.
        value: ``DbType`` payload.
    """

    kind: Literal["null"]
    value: DbType


class DbValueBoolean(TypedDict):
    """``DbValue`` member ``boolean``.

    Attributes:
        kind: Always ``"boolean"``.
        value: ``Bool`` payload.
    """

    kind: Literal["boolean"]
    value: bool


class DbValueInt64(TypedDict):
    """``DbValue`` member ``int64``.

    Attributes:
        kind: Always ``"int64"``.
        value: ``Int64`` payload.
    """

    kind: Literal["int64"]
    value: int


class DbValueFloat64(TypedDict):
    """``DbValue`` member ``float64``.

    Attributes:
        kind: Always ``"float64"``.
        value: ``Float64`` payload.
    """

    kind: Literal["float64"]
    value: float


class DbValueText(TypedDict):
    """``DbValue`` member ``text``.

    Attributes:
        kind: Always ``"text"``.
        value: ``Text`` payload.
    """

    kind: Literal["text"]
    value: str


class DbValueBytes(TypedDict):
    """``DbValue`` member ``bytes``.

    Attributes:
        kind: Always ``"bytes"``.
        value: ``Data`` payload.
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
"""``DbValue`` union."""


class DbColumn(TypedDict):
    """(undocumented)

    Attributes:
        name: name
        dbType: dbType
    """

    name: str
    dbType: DbType


class DbRow(TypedDict):
    """(undocumented)

    Attributes:
        values: values
    """

    values: list[DbValue]


class DbStatement(TypedDict):
    """(undocumented)

    Attributes:
        sql: sql
        parameters: parameters
        kind: kind
        maxRows: maxRows
        resultSelection: resultSelection
    """

    sql: str
    parameters: list[DbValue]
    kind: DbStatementKind
    maxRows: int
    resultSelection: DbResultSelection


class ExecuteRequest(TypedDict):
    """(undocumented)

    Attributes:
        operationId: operationId
        requestHash: requestHash
        statements: statements
        deadlineUnixMs: deadlineUnixMs
    """

    operationId: str
    requestHash: str
    statements: list[DbStatement]
    deadlineUnixMs: int


class SqlSpan(TypedDict):
    """(undocumented)

    Attributes:
        start: start
        end: end
    """

    start: int
    end: int


class TextCollateSite(TypedDict):
    """(undocumented)

    Attributes:
        span: span
    """

    span: SqlSpan


class IntegerArithSite(TypedDict):
    """(undocumented)

    Attributes:
        full: full
        lhs: lhs
        rhs: rhs
        kind: kind
    """

    full: SqlSpan
    lhs: SqlSpan
    rhs: SqlSpan
    kind: IntegerArithKind


class PhysicalAccess(TypedDict):
    """(undocumented)

    Attributes:
        table: table
        column: Empty = table presence only; "*" = projection wildcard.
    """

    table: str
    column: str


class ResolvedAssignment(TypedDict):
    """(undocumented)

    Attributes:
        table: table
        column: column
        dest: dest
        source: source
    """

    table: str
    column: str
    dest: ResolvedSqlType
    source: ResolvedSqlType


class NamedSqlType(TypedDict):
    """(undocumented)

    Attributes:
        name: name
        sqlType: sqlType
    """

    name: str
    sqlType: ResolvedSqlType


class ColumnReference(TypedDict):
    """(undocumented)

    Attributes:
        refTable: refTable
        refColumns: refColumns
    """

    refTable: str
    refColumns: list[str]


class OptionalColumnReferenceNone(TypedDict):
    """``OptionalColumnReference`` member ``none``.

    Attributes:
        kind: Always ``"none"``.
    """

    kind: Literal["none"]


class OptionalColumnReferenceSome(TypedDict):
    """``OptionalColumnReference`` member ``some``.

    Attributes:
        kind: Always ``"some"``.
        value: ``ColumnReference`` payload.
    """

    kind: Literal["some"]
    value: ColumnReference


OptionalColumnReference = Union[
    OptionalColumnReferenceNone,
    OptionalColumnReferenceSome,
]
"""``OptionalColumnReference`` union."""


class ForeignKeyConstraint(TypedDict):
    """(undocumented)

    Attributes:
        columns: columns
        refTable: refTable
        refColumns: refColumns
    """

    columns: list[str]
    refTable: str
    refColumns: list[str]


class TableConstraintPrimaryKey(TypedDict):
    """``TableConstraint`` member ``primaryKey``.

    Attributes:
        kind: Always ``"primaryKey"``.
        value: ``List(Text)`` payload.
    """

    kind: Literal["primaryKey"]
    value: list[str]


class TableConstraintUnique(TypedDict):
    """``TableConstraint`` member ``unique``.

    Attributes:
        kind: Always ``"unique"``.
        value: ``List(Text)`` payload.
    """

    kind: Literal["unique"]
    value: list[str]


class TableConstraintCheck(TypedDict):
    """``TableConstraint`` member ``check``.

    Attributes:
        kind: Always ``"check"``.
        value: ``Text`` payload.
    """

    kind: Literal["check"]
    value: str


class TableConstraintForeignKey(TypedDict):
    """``TableConstraint`` member ``foreignKey``.

    Attributes:
        kind: Always ``"foreignKey"``.
        value: ``ForeignKeyConstraint`` payload.
    """

    kind: Literal["foreignKey"]
    value: ForeignKeyConstraint


TableConstraint = Union[
    TableConstraintPrimaryKey,
    TableConstraintUnique,
    TableConstraintCheck,
    TableConstraintForeignKey,
]
"""``TableConstraint`` union."""


class CreateTableSchema(TypedDict):
    """(undocumented)

    Attributes:
        table: table
        columns: columns
        identityColumn: identityColumn
        columnNotNull: columnNotNull
        columnUnique: columnUnique
        columnPrimaryKey: columnPrimaryKey
        columnDefaults: columnDefaults
        columnChecks: columnChecks
        columnReferences: columnReferences
        tableConstraints: tableConstraints
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
    """(undocumented)

    Attributes:
        schema: schema
        fingerprint: fingerprint
        noop: noop
    """

    schema: CreateTableSchema
    fingerprint: str
    noop: bool


class SchemaActionNone(TypedDict):
    """``SchemaAction`` member ``none``.

    Attributes:
        kind: Always ``"none"``.
    """

    kind: Literal["none"]


class SchemaActionCreate(TypedDict):
    """``SchemaAction`` member ``create``.

    Attributes:
        kind: Always ``"create"``.
        value: ``SchemaCreate`` payload.
    """

    kind: Literal["create"]
    value: SchemaCreate


class SchemaActionDrop(TypedDict):
    """``SchemaAction`` member ``drop``.

    Attributes:
        kind: Always ``"drop"``.
        value: ``Text`` payload.
    """

    kind: Literal["drop"]
    value: str


SchemaAction = Union[
    SchemaActionNone,
    SchemaActionCreate,
    SchemaActionDrop,
]
"""``SchemaAction`` union."""


class ResolvedStatement(TypedDict):
    """(undocumented)

    Attributes:
        statementHash: statementHash
        outputColumns: outputColumns
        physicalAccesses: physicalAccesses
        assignments: assignments
        textCollateSites: textCollateSites
        integerArithSites: integerArithSites
        functions: functions
        schemaAction: schemaAction
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
    """(undocumented)

    Attributes:
        guestLen: guestLen
        guestHash: guestHash
    """

    guestLen: int
    guestHash: str


class AdapterStatement(TypedDict):
    """(undocumented)

    Attributes:
        sql: sql
        parameters: parameters
        kind: kind
        maxRows: maxRows
        resultSelection: resultSelection
        proof: proof
    """

    sql: str
    parameters: list[DbValue]
    kind: DbStatementKind
    maxRows: int
    resultSelection: DbResultSelection
    proof: ResolvedStatement


class AdapterExecuteRequest(TypedDict):
    """(undocumented)

    Attributes:
        operationId: operationId
        requestHash: requestHash
        statements: statements
        deadlineUnixMs: deadlineUnixMs
        isolation: isolation
        receipt: receipt
    """

    operationId: str
    requestHash: str
    statements: list[AdapterStatement]
    deadlineUnixMs: int
    isolation: IsolationReq
    receipt: AdapterReceipt


class StatementResult(TypedDict):
    """(undocumented)

    Attributes:
        rows: rows
        columns: columns
        rowsAffected: rowsAffected
    """

    rows: list[DbRow]
    columns: list[DbColumn]
    rowsAffected: int


class DbTiming(TypedDict):
    """(undocumented)

    Attributes:
        attemptElapsedUs: attemptElapsedUs
        dbExecutionUs: dbExecutionUs
        dbTimingSource: dbTimingSource
    """

    attemptElapsedUs: int
    dbExecutionUs: int
    dbTimingSource: str


class ExecuteReply(TypedDict):
    """(undocumented)

    Attributes:
        operationId: operationId
        statements: statements
        timing: timing
    """

    operationId: str
    statements: list[StatementResult]
    timing: DbTiming


class ExecuteResultReplyOk(TypedDict):
    """``ExecuteResultReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``ExecuteReply`` payload.
    """

    kind: Literal["ok"]
    value: ExecuteReply


class ExecuteResultReplyErr(TypedDict):
    """``ExecuteResultReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


ExecuteResultReply = Union[
    ExecuteResultReplyOk,
    ExecuteResultReplyErr,
]
"""``ExecuteResultReply`` union."""


class DbCapabilities(TypedDict):
    """Semantic SQL-contract advertisement. Diagnostic engine identity is not
    part of the capability plane — see `DbBootstrap`.

    Attributes:
        sqlContractVersion: sqlContractVersion
        atomicBatch: atomicBatch
        returning: returning
        affectedRows: affectedRows
        schemaMigrations: schemaMigrations
        cancellation: cancellation
        timing: timing
        maxBinds: maxBinds
        maxStatements: maxStatements
        maxResultRows: maxResultRows
        maxPayloadBytes: maxPayloadBytes
        maxResultBytes: maxResultBytes
        maxCellBytes: maxCellBytes
        maxRequestBytes: maxRequestBytes
        maxAtomicResultBytes: maxAtomicResultBytes
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

    Attributes:
        kind: Always ``"ok"``.
        value: ``DbBootstrap`` payload.
    """

    kind: Literal["ok"]
    value: DbBootstrap


class DbBootstrapReplyErr(TypedDict):
    """``DbBootstrapReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


DbBootstrapReply = Union[
    DbBootstrapReplyOk,
    DbBootstrapReplyErr,
]
"""``DbBootstrapReply`` union."""


class DbBootstrap(TypedDict):
    """(undocumented)

    Attributes:
        engine: Diagnostic physical engine name. Hosts must not admit or generate SQL
            from this value. Any string is valid.
    """

    engine: str


class DbCapabilitiesReplyOk(TypedDict):
    """``DbCapabilitiesReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``DbCapabilities`` payload.
    """

    kind: Literal["ok"]
    value: DbCapabilities


class DbCapabilitiesReplyErr(TypedDict):
    """``DbCapabilitiesReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


DbCapabilitiesReply = Union[
    DbCapabilitiesReplyOk,
    DbCapabilitiesReplyErr,
]
"""``DbCapabilitiesReply`` union."""


class Database(Protocol):
    """``Database`` capability."""

    async def open_session(self) -> AdapterSessionReply:
        """``Database.openSession``.

        Returns:
            result
        """
        ...


class IdentityHighWater(TypedDict):
    """Adapter-private identity high-water (sqlite_sequence / bookclerk_identity).
    Column names live in the canonical backup schema, not this catalog.

    Attributes:
        table: table
        last: last
    """

    table: str
    last: int


class IdentityExportReplyOk(TypedDict):
    """``IdentityExportReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``List(IdentityHighWater)`` payload.
    """

    kind: Literal["ok"]
    value: list[IdentityHighWater]


class IdentityExportReplyErr(TypedDict):
    """``IdentityExportReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


IdentityExportReply = Union[
    IdentityExportReplyOk,
    IdentityExportReplyErr,
]
"""``IdentityExportReply`` union."""


class UserRelationsReplyOk(TypedDict):
    """``UserRelationsReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``List(Text)`` payload.
    """

    kind: Literal["ok"]
    value: list[str]


class UserRelationsReplyErr(TypedDict):
    """``UserRelationsReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


UserRelationsReply = Union[
    UserRelationsReplyOk,
    UserRelationsReplyErr,
]
"""``UserRelationsReply`` union."""


class AdapterDatabaseSession(Protocol):
    """Host ↔ database adapter plugin. Capability negotiation + typed execute only."""

    async def capabilities(self) -> DbCapabilitiesReply:
        """``AdapterDatabaseSession.capabilities``.

        Returns:
            result
        """
        ...

    async def execute(self, request: AdapterExecuteRequest) -> ExecuteResultReply:
        """Canonical SQL + required structured proofs (not JSON).

        Args:
            request: request

        Returns:
            result
        """
        ...

    async def close(self) -> EmptyReply:
        """``AdapterDatabaseSession.close``.

        Returns:
            result
        """
        ...

    async def bootstrap(self) -> DbBootstrapReply:
        """Bootstrap-only SeaORM proxy metadata (not part of DbCapabilities).

        Returns:
            result
        """
        ...

    async def export_identity(self) -> IdentityExportReply:
        """Snapshot/identity/restore primitives (not a SQL dialect API).

        Returns:
            result
        """
        ...

    async def import_identity(self, rows: list[IdentityHighWater]) -> EmptyReply:
        """``AdapterDatabaseSession.importIdentity``.

        Args:
            rows: rows

        Returns:
            result
        """
        ...

    async def list_user_relations(self) -> UserRelationsReply:
        """``AdapterDatabaseSession.listUserRelations``.

        Returns:
            result
        """
        ...

    async def prepare_unit_restore(self) -> EmptyReply:
        """``AdapterDatabaseSession.prepareUnitRestore``.

        Returns:
            result
        """
        ...

    async def drop_user_relations(self, names: list[str]) -> EmptyReply:
        """``AdapterDatabaseSession.dropUserRelations``.

        Args:
            names: names

        Returns:
            result
        """
        ...

    async def assert_restore_constraints(self) -> EmptyReply:
        """``AdapterDatabaseSession.assertRestoreConstraints``.

        Returns:
            result
        """
        ...


class GuestDatabase(Protocol):
    """Host-granted SQL for job plugin authors. SDK `DatabaseBinding` mirrors the
    Cloudflare Workers D1 surface (`prepare`/`bind`/`run`/`first`/`all`/`raw`,
    `batch`, `exec`) over this typed `execute` transport; wire types stay Cap'n
    `ExecuteRequest`/`ExecuteReply`.
    """

    async def execute(self, request: ExecuteRequest) -> ExecuteResultReply:
        """``GuestDatabase.execute``.

        Args:
            request: request

        Returns:
            result
        """
        ...

    async def close(self) -> EmptyReply:
        """``GuestDatabase.close``.

        Returns:
            result
        """
        ...


class PluginMigrationOpSchema(TypedDict):
    """``PluginMigrationOp`` member ``schema``.

    Attributes:
        kind: Always ``"schema"``.
        value: ``Text`` payload.
    """

    kind: Literal["schema"]
    value: str


class PluginMigrationOpData(TypedDict):
    """``PluginMigrationOp`` member ``data``.

    Attributes:
        kind: Always ``"data"``.
        value: ``Text`` payload.
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
        id: id
        operations: at most `maxPluginMigrationOps`
    """

    id: str
    operations: list[PluginMigrationOp]


class PluginMigrationsOk(TypedDict):
    """(undocumented)

    Attributes:
        migrations: At most `maxListPage` entries; aggregate id+SQL bytes at most
            `maxPluginMigrationRegistrationBytes`; total operations at most
            `maxPluginMigrationTotalOps`.
    """

    migrations: list[PluginMigration]


class PluginMigrationsReplyOk(TypedDict):
    """``PluginMigrationsReply`` member ``ok``.

    Attributes:
        kind: Always ``"ok"``.
        value: ``PluginMigrationsOk`` payload.
    """

    kind: Literal["ok"]
    value: PluginMigrationsOk


class PluginMigrationsReplyErr(TypedDict):
    """``PluginMigrationsReply`` member ``err``.

    Attributes:
        kind: Always ``"err"``.
        value: ``PluginError`` payload.
    """

    kind: Literal["err"]
    value: PluginError


PluginMigrationsReply = Union[
    PluginMigrationsReplyOk,
    PluginMigrationsReplyErr,
]
"""``PluginMigrationsReply`` union."""


class BookclerkPlugin(Protocol):
    """``BookclerkPlugin`` capability."""

    async def describe(self) -> DescribeReply:
        """``BookclerkPlugin.describe``.

        Returns:
            result
        """
        ...

    async def destination(self, context: DestinationContext) -> DestinationReply:
        """``BookclerkPlugin.destination``.

        Args:
            context: context

        Returns:
            result
        """
        ...

    async def source(self, context: SourceContext) -> SourceReply:
        """``BookclerkPlugin.source``.

        Args:
            context: context

        Returns:
            result
        """
        ...

    async def worker(self, context: WorkerContext) -> WorkerReply:
        """``BookclerkPlugin.worker``.

        Args:
            context: context

        Returns:
            result
        """
        ...

    async def shutdown(self) -> EmptyReply:
        """``BookclerkPlugin.shutdown``.

        Returns:
            result
        """
        ...

    async def content_source(self, context: ContentSourceContext) -> ContentSourceReply:
        """``BookclerkPlugin.contentSource``.

        Args:
            context: context

        Returns:
            result
        """
        ...

    async def integration(self, context: IntegrationContext) -> IntegrationReply:
        """``BookclerkPlugin.integration``.

        Args:
            context: context

        Returns:
            result
        """
        ...

    async def database(self, context: DatabaseContext) -> DatabaseReply:
        """``BookclerkPlugin.database``.

        Args:
            context: context

        Returns:
            result
        """
        ...

    async def cli_describe(self) -> JsonReply:
        """``BookclerkPlugin.cliDescribe``.

        Returns:
            result
        """
        ...

    async def cli_invoke(self, params_json: str) -> JsonReply:
        """``BookclerkPlugin.cliInvoke``.

        Args:
            params_json: params_json

        Returns:
            result
        """
        ...

    async def oidc_clients(self) -> OidcClientsReply:
        """Plugin-provided OIDC AS client templates. Empty list when unused.

        Returns:
            result
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
            binding: binding

        Returns:
            result
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
