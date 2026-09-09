"""GENERATED FILE - do not edit. Run `python3 scripts/gen-plugin-abi.py --write` after changing crates/bookclerk-plugin-abi/schema/plugin.capnp.

Python projection of the product ABI constants and enum ordinal tables
declared in ``crates/bookclerk-plugin-abi/schema/plugin.capnp``.
"""

from __future__ import annotations

PRODUCT_API_VERSION: int = 2
"""apiVersion"""

MAX_SCALAR_BYTES: int = 262144
"""maxScalarBytes"""

MAX_STREAM_WINDOW_BYTES: int = 1048576
"""maxStreamWindowBytes"""

MAX_LIST_PAGE: int = 256
"""maxListPage"""

MAX_CHECKPOINT_BYTES: int = 65536
"""maxCheckpointBytes"""

MAX_IDENTIFIER_BYTES: int = 64
"""maxIdentifierBytes"""

MAX_CONFIG_PAYLOAD_BYTES: int = 65536
"""maxConfigPayloadBytes"""

MAX_EVENT_PAYLOAD_BYTES: int = 65536
"""maxEventPayloadBytes"""

MAX_PLUGIN_MIGRATION_OPS: int = 256
"""Plugin `databaseMigrations` is a startup-time scalar (not a stream). Count is
`maxListPage`. Each SQL text is `maxScalarBytes`. Ops-per-migration, total
operations across the registration, and the aggregate UTF-8 bytes of ids +
SQL have dedicated caps so a jailed guest cannot force unbounded host
allocation before semantic proof. `maxPluginMigrationRegistrationBytes` is
id+SQL text only; `maxPluginMigrationTotalOps` bounds the structural object
graph (2048: enough for realistic histories, including 256 migrations of ~8
ops or 8 migrations at the per-migration cap, and far below 256×256).
"""

MAX_PLUGIN_MIGRATION_TOTAL_OPS: int = 2048
"""maxPluginMigrationTotalOps"""

MAX_PLUGIN_MIGRATION_REGISTRATION_BYTES: int = 262144
"""maxPluginMigrationRegistrationBytes"""

FEATURE_SCALAR_LIMITS: str = "rpc.scalarLimits"
"""Negotiable `rpcFeatures` wire names (see `PluginDescribe.rpcFeatures`)."""

FEATURE_STREAMS: str = "rpc.streams"
"""featureStreams"""

FEATURE_STORAGE_COPY: str = "storage.copy"
"""featureStorageCopy"""

PLUGIN_ERROR_CODES: tuple[str, ...] = ("invalid_params", "unauthorized", "forbidden", "not_found", "unavailable", "unsupported", "internal", "payload_too_large", "deadline_exceeded", "invalid_cursor", "cancelled", "conflict")
"""Stable `PluginError.code` strings. Unknown future codes are forwarded
as-is; SDKs surface them as a local `unknown` while keeping the raw wire
code.

Wire strings of ``PluginErrorCode`` in ordinal order (snake_case ``Text`` codes).
"""

CLI_ARG_KINDS: tuple[str, ...] = ("string", "bool", "int", "path")
"""Value kind for a `CliArgSpec` (wire lowercase: "string" / "bool" / ...).

Wire strings of ``CliArgKind`` in ordinal order (snake_case ``Text`` codes).
"""

DB_TYPES: tuple[str, ...] = ("unspecified", "bool", "int64", "float64", "text", "bytes")
"""Universal database cell/parameter domain. Engine-native arrays, enums,
unsigned integers, and JSON text sentinels are not baseline ABI values.

Ordinal-ordered ``DbType`` wire names (index = Cap'n Proto ordinal).
"""

DB_STATEMENT_KINDS: tuple[str, ...] = ("execute", "select", "returning")
"""Ordinal-ordered ``DbStatementKind`` wire names (index = Cap'n Proto ordinal)."""

DB_RESULT_SELECTIONS: tuple[str, ...] = ("discard", "affectedRows", "rows")
"""Ordinal-ordered ``DbResultSelection`` wire names (index = Cap'n Proto ordinal)."""

ISOLATION_REQS: tuple[str, ...] = ("atomicBatch", "nestedSavepoint", "consistentSnapshot")
"""Host → adapter execute. GuestDatabase stays ExecuteRequest-only.

Ordinal-ordered ``IsolationReq`` wire names (index = Cap'n Proto ordinal).
"""

RESOLVED_SQL_TYPES: tuple[str, ...] = ("integer", "real", "text", "blob", "boolean", "null")
"""Ordinal-ordered ``ResolvedSqlType`` wire names (index = Cap'n Proto ordinal)."""

INTEGER_ARITH_KINDS: tuple[str, ...] = ("add", "sub", "mul", "abs")
"""Ordinal-ordered ``IntegerArithKind`` wire names (index = Cap'n Proto ordinal)."""

__all__ = [
    "PRODUCT_API_VERSION",
    "MAX_SCALAR_BYTES",
    "MAX_STREAM_WINDOW_BYTES",
    "MAX_LIST_PAGE",
    "MAX_CHECKPOINT_BYTES",
    "MAX_IDENTIFIER_BYTES",
    "MAX_CONFIG_PAYLOAD_BYTES",
    "MAX_EVENT_PAYLOAD_BYTES",
    "MAX_PLUGIN_MIGRATION_OPS",
    "MAX_PLUGIN_MIGRATION_TOTAL_OPS",
    "MAX_PLUGIN_MIGRATION_REGISTRATION_BYTES",
    "FEATURE_SCALAR_LIMITS",
    "FEATURE_STREAMS",
    "FEATURE_STORAGE_COPY",
    "PLUGIN_ERROR_CODES",
    "CLI_ARG_KINDS",
    "DB_TYPES",
    "DB_STATEMENT_KINDS",
    "DB_RESULT_SELECTIONS",
    "ISOLATION_REQS",
    "RESOLVED_SQL_TYPES",
    "INTEGER_ARITH_KINDS",
]
