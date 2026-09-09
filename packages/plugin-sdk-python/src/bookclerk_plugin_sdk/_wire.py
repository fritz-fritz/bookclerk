"""GENERATED FILE - do not edit. Run `python3 scripts/gen-plugin-abi.py --write` after changing crates/bookclerk-plugin-abi/schema/plugin.capnp.

Unpacked single-segment Cap'n Proto codecs for every struct and method
envelope in ``plugin.capnp``. Layout numbers come from
``schema/plugin.layout.json`` (Cap'n Proto compiler output), not from
hand-derived tables. Pointer targets are allocated in declaration order,
depth first, matching the ``capnpc-rust`` builder so Rust, TypeScript, and
Python produce byte-identical messages for the same value.
"""

from __future__ import annotations

from typing import Any

from . import _abi as A
from ._capnp import (
    _CapnpMessage,
    _CapnpReader,
    _CapnpStruct,
    _CapTable,
    _StructReader,
    NO_CAPS,
)


class _Codec:
    """Struct codec over the runtime Cap'n builders/readers."""

    def __init__(self, data_words: int, pointer_count: int, write: Any, read: Any) -> None:
        self.data_words = data_words
        self.pointer_count = pointer_count
        self.write = write
        self.read = read


def _encode_message(codec: _Codec, value: Any, caps: _CapTable = NO_CAPS) -> bytes:
    """Encode ``value`` as a standalone unpacked Cap'n message."""
    msg = _CapnpMessage()
    codec.write(msg.init_root(codec.data_words, codec.pointer_count), value, caps)
    return msg.finish()


def _decode_message(codec: _Codec, data: bytes, caps: _CapTable = NO_CAPS) -> Any:
    """Decode a standalone unpacked Cap'n message."""
    return codec.read(_CapnpReader(data).root(codec.data_words, codec.pointer_count), caps)


def _ord(table: tuple[str, ...], name: str, enum_name: str) -> int:
    try:
        return table.index(name)
    except ValueError as exc:
        raise ValueError(f"unknown {enum_name} value: {name}") from exc


def _from_ord(table: tuple[str, ...], ordinal: int, enum_name: str) -> str:
    if ordinal < 0 or ordinal >= len(table):
        raise ValueError(f"unknown {enum_name} ordinal: {ordinal}")
    return table[ordinal]


def _write_scalar_limits(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_u32(0, v["maxScalarBytes"])
    s.set_u32(1, v["maxStreamWindowBytes"])
    s.set_u32(2, v["maxListPage"])


def _read_scalar_limits(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "maxScalarBytes": s.get_u32(0),
        "maxStreamWindowBytes": s.get_u32(1),
        "maxListPage": s.get_u32(2),
    }


_scalar_limits_codec = _Codec(2, 0, _write_scalar_limits, _read_scalar_limits)
"""Wire codec for ``ScalarLimits`` (2 data words, 0 pointers)."""


def _write_plugin_error(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["code"])
    s.set_text(1, v["message"])


def _read_plugin_error(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "code": s.get_text(0),
        "message": s.get_text(1),
    }


_plugin_error_codec = _Codec(0, 2, _write_plugin_error, _read_plugin_error)
"""Wire codec for ``PluginError`` (0 data words, 2 pointers)."""


def _write_object_metadata(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["key"])
    s.set_u64(0, v["size"])
    s.set_text(1, v["contentType"])
    s.set_text(2, v["etag"])
    s.set_data(3, v["sha256"])


def _read_object_metadata(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "key": s.get_text(0),
        "size": s.get_u64(0),
        "contentType": s.get_text(1),
        "etag": s.get_text(2),
        "sha256": s.get_data(3),
    }


_object_metadata_codec = _Codec(1, 4, _write_object_metadata, _read_object_metadata)
"""Wire codec for ``ObjectMetadata`` (1 data words, 4 pointers)."""


def _write_object_info(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["key"])
    s.set_u64(0, v["size"])


def _read_object_info(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "key": s.get_text(0),
        "size": s.get_u64(0),
    }


_object_info_codec = _Codec(1, 1, _write_object_info, _read_object_info)
"""Wire codec for ``ObjectInfo`` (1 data words, 1 pointers)."""


def _write_list_options(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["prefix"])
    s.set_text(1, v["cursor"])
    s.set_u32(0, v["limit"])


def _read_list_options(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "prefix": s.get_text(0),
        "cursor": s.get_text(1),
        "limit": s.get_u32(0),
    }


_list_options_codec = _Codec(1, 2, _write_list_options, _read_list_options)
"""Wire codec for ``ListOptions`` (1 data words, 2 pointers)."""


def _write_list_page(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    items = s.init_struct_list(0, len(v["objects"]), 1, 1)
    for item, elem in zip(items, v["objects"], strict=True):
        _object_info_codec.write(item, elem, caps)
    s.set_text(1, v["nextCursor"])


def _read_list_page(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "objects": [_object_info_codec.read(item, caps) for item in s.get_struct_list(0, 1, 1)],
        "nextCursor": s.get_text(1),
    }


_list_page_codec = _Codec(0, 2, _write_list_page, _read_list_page)
"""Wire codec for ``ListPage`` (0 data words, 2 pointers)."""


def _write_byte_range(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_u64(0, v["offset"])
    s.set_u64(1, v["length"])


def _read_byte_range(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "offset": s.get_u64(0),
        "length": s.get_u64(1),
    }


_byte_range_codec = _Codec(2, 0, _write_byte_range, _read_byte_range)
"""Wire codec for ``ByteRange`` (2 data words, 0 pointers)."""


def _write_read_options(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _byte_range_codec.write(s.init_struct(0, 2, 0), v["range"], caps)


def _read_read_options(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "range": _byte_range_codec.read(s.get_struct(0, 2, 0), caps),
    }


_read_options_codec = _Codec(0, 1, _write_read_options, _read_read_options)
"""Wire codec for ``ReadOptions`` (0 data words, 1 pointers)."""


def _write_write_options(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["contentType"])
    s.set_u64(0, v["contentLength"])
    s.set_data(1, v["sha256"])
    s.set_text(2, v["commitToken"])
    s.set_bool(64, v["stageOnly"])


def _read_write_options(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "contentType": s.get_text(0),
        "contentLength": s.get_u64(0),
        "sha256": s.get_data(1),
        "commitToken": s.get_text(2),
        "stageOnly": s.get_bool(64),
    }


_write_options_codec = _Codec(2, 3, _write_write_options, _read_write_options)
"""Wire codec for ``WriteOptions`` (2 data words, 3 pointers)."""


def _write_put_result(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["key"])
    s.set_u64(0, v["bytesWritten"])
    s.set_text(1, v["etag"])
    s.set_data(2, v["sha256"])


def _read_put_result(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "key": s.get_text(0),
        "bytesWritten": s.get_u64(0),
        "etag": s.get_text(1),
        "sha256": s.get_data(2),
    }


_put_result_codec = _Codec(1, 3, _write_put_result, _read_put_result)
"""Wire codec for ``PutResult`` (1 data words, 3 pointers)."""


def _write_copy_result(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_u64(0, v["bytesCopied"])


def _read_copy_result(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "bytesCopied": s.get_u64(0),
    }


_copy_result_codec = _Codec(1, 0, _write_copy_result, _read_copy_result)
"""Wire codec for ``CopyResult`` (1 data words, 0 pointers)."""


def _write_plugin_describe(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_u32(0, v["apiVersion"])
    s.set_text(0, v["id"])
    s.set_text(1, v["kind"])
    s.set_text(2, v["displayName"])
    s.set_text_list(3, v["rpcFeatures"])
    _scalar_limits_codec.write(s.init_struct(4, 2, 0), v["scalarLimits"], caps)
    s.set_text_list(5, v["supportedRoles"])
    s.set_text(6, v["metadataJson"])


def _read_plugin_describe(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "apiVersion": s.get_u32(0),
        "id": s.get_text(0),
        "kind": s.get_text(1),
        "displayName": s.get_text(2),
        "rpcFeatures": s.get_text_list(3),
        "scalarLimits": _scalar_limits_codec.read(s.get_struct(4, 2, 0), caps),
        "supportedRoles": s.get_text_list(5),
        "metadataJson": s.get_text(6),
    }


_plugin_describe_codec = _Codec(1, 7, _write_plugin_describe, _read_plugin_describe)
"""Wire codec for ``PluginDescribe`` (1 data words, 7 pointers)."""


def _write_oidc_client_template(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["clientId"])
    s.set_text(1, v["displayName"])
    s.set_text(2, v["callbackPath"])
    s.set_bool(0, v["publicClient"])
    s.set_text_list(3, v["defaultScopes"])
    s.set_bool(1, v["issueRefreshToken"])
    s.set_text(4, v["originConfigKey"])


def _read_oidc_client_template(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "clientId": s.get_text(0),
        "displayName": s.get_text(1),
        "callbackPath": s.get_text(2),
        "publicClient": s.get_bool(0),
        "defaultScopes": s.get_text_list(3),
        "issueRefreshToken": s.get_bool(1),
        "originConfigKey": s.get_text(4),
    }


_oidc_client_template_codec = _Codec(1, 5, _write_oidc_client_template, _read_oidc_client_template)
"""Wire codec for ``OidcClientTemplate`` (1 data words, 5 pointers)."""


def _write_oidc_clients_ok(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    items = s.init_struct_list(0, len(v["clients"]), 1, 5)
    for item, elem in zip(items, v["clients"], strict=True):
        _oidc_client_template_codec.write(item, elem, caps)


def _read_oidc_clients_ok(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "clients": [_oidc_client_template_codec.read(item, caps) for item in s.get_struct_list(0, 1, 5)],
    }


_oidc_clients_ok_codec = _Codec(0, 1, _write_oidc_clients_ok, _read_oidc_clients_ok)
"""Wire codec for ``OidcClientsOk`` (0 data words, 1 pointers)."""


def _write_oidc_clients_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        _oidc_clients_ok_codec.write(s.init_struct(0, 0, 1), v["value"], caps)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown OidcClientsReply union member: {kind}")


def _read_oidc_clients_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": _oidc_clients_ok_codec.read(s.get_struct(0, 0, 1), caps)}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown OidcClientsReply union member: {disc}")


_oidc_clients_reply_codec = _Codec(1, 1, _write_oidc_clients_reply, _read_oidc_clients_reply)
"""Wire codec for ``OidcClientsReply`` (1 data words, 1 pointers)."""


def _write_extensible_config(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_u32(0, v["schemaVersion"])
    s.set_text(0, v["mediaType"])
    s.set_data(1, v["payload"])


def _read_extensible_config(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "schemaVersion": s.get_u32(0),
        "mediaType": s.get_text(0),
        "payload": s.get_data(1),
    }


_extensible_config_codec = _Codec(1, 2, _write_extensible_config, _read_extensible_config)
"""Wire codec for ``ExtensibleConfig`` (1 data words, 2 pointers)."""


def _write_destination_context(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["json"])
    _extensible_config_codec.write(s.init_struct(1, 1, 2), v["config"], caps)


def _read_destination_context(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "json": s.get_text(0),
        "config": _extensible_config_codec.read(s.get_struct(1, 1, 2), caps),
    }


_destination_context_codec = _Codec(0, 2, _write_destination_context, _read_destination_context)
"""Wire codec for ``DestinationContext`` (0 data words, 2 pointers)."""


def _write_source_context(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["json"])
    _extensible_config_codec.write(s.init_struct(1, 1, 2), v["config"], caps)


def _read_source_context(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "json": s.get_text(0),
        "config": _extensible_config_codec.read(s.get_struct(1, 1, 2), caps),
    }


_source_context_codec = _Codec(0, 2, _write_source_context, _read_source_context)
"""Wire codec for ``SourceContext`` (0 data words, 2 pointers)."""


def _write_worker_context(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["jobId"])
    s.set_text(1, v["json"])
    _extensible_config_codec.write(s.init_struct(2, 1, 2), v["config"], caps)


def _read_worker_context(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "jobId": s.get_text(0),
        "json": s.get_text(1),
        "config": _extensible_config_codec.read(s.get_struct(2, 1, 2), caps),
    }


_worker_context_codec = _Codec(0, 3, _write_worker_context, _read_worker_context)
"""Wire codec for ``WorkerContext`` (0 data words, 3 pointers)."""


def _write_content_source_context(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["json"])
    _extensible_config_codec.write(s.init_struct(1, 1, 2), v["config"], caps)


def _read_content_source_context(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "json": s.get_text(0),
        "config": _extensible_config_codec.read(s.get_struct(1, 1, 2), caps),
    }


_content_source_context_codec = _Codec(0, 2, _write_content_source_context, _read_content_source_context)
"""Wire codec for ``ContentSourceContext`` (0 data words, 2 pointers)."""


def _write_integration_context(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["json"])
    _extensible_config_codec.write(s.init_struct(1, 1, 2), v["config"], caps)


def _read_integration_context(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "json": s.get_text(0),
        "config": _extensible_config_codec.read(s.get_struct(1, 1, 2), caps),
    }


_integration_context_codec = _Codec(0, 2, _write_integration_context, _read_integration_context)
"""Wire codec for ``IntegrationContext`` (0 data words, 2 pointers)."""


def _write_database_context(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["json"])
    _extensible_config_codec.write(s.init_struct(1, 1, 2), v["config"], caps)


def _read_database_context(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "json": s.get_text(0),
        "config": _extensible_config_codec.read(s.get_struct(1, 1, 2), caps),
    }


_database_context_codec = _Codec(0, 2, _write_database_context, _read_database_context)
"""Wire codec for ``DatabaseContext`` (0 data words, 2 pointers)."""


def _write_job_invocation(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_u32(0, v["payloadSchemaVersion"])
    s.set_text(0, v["invocationId"])
    s.set_text(1, v["commandType"])
    s.set_text(2, v["payloadJson"])
    s.set_text(3, v["idempotencyKey"])
    s.set_u32(1, v["attempt"])
    s.set_text(4, v["correlationId"])
    s.set_text(5, v["causationId"])
    s.set_u64(1, v["deadlineUnixMs"])
    s.set_text(6, v["checkpointJson"])
    s.set_u32(4, v["checkpointSchemaVersion"])
    s.set_u32(5, v["invocationSequence"])
    s.set_text(7, v["stepId"])


def _read_job_invocation(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "payloadSchemaVersion": s.get_u32(0),
        "invocationId": s.get_text(0),
        "commandType": s.get_text(1),
        "payloadJson": s.get_text(2),
        "idempotencyKey": s.get_text(3),
        "attempt": s.get_u32(1),
        "correlationId": s.get_text(4),
        "causationId": s.get_text(5),
        "deadlineUnixMs": s.get_u64(1),
        "checkpointJson": s.get_text(6),
        "checkpointSchemaVersion": s.get_u32(4),
        "invocationSequence": s.get_u32(5),
        "stepId": s.get_text(7),
    }


_job_invocation_codec = _Codec(3, 8, _write_job_invocation, _read_job_invocation)
"""Wire codec for ``JobInvocation`` (3 data words, 8 pointers)."""


def _write_completed_outcome(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["message"])
    s.set_u64(0, v["bytesCopied"])


def _read_completed_outcome(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "message": s.get_text(0),
        "bytesCopied": s.get_u64(0),
    }


_completed_outcome_codec = _Codec(1, 1, _write_completed_outcome, _read_completed_outcome)
"""Wire codec for ``CompletedOutcome`` (1 data words, 1 pointers)."""


def _write_retryable_outcome(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["message"])
    s.set_u64(0, v["retryAfterUnixMs"])


def _read_retryable_outcome(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "message": s.get_text(0),
        "retryAfterUnixMs": s.get_u64(0),
    }


_retryable_outcome_codec = _Codec(1, 1, _write_retryable_outcome, _read_retryable_outcome)
"""Wire codec for ``RetryableOutcome`` (1 data words, 1 pointers)."""


def _write_rejected_outcome(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["message"])


def _read_rejected_outcome(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "message": s.get_text(0),
    }


_rejected_outcome_codec = _Codec(0, 1, _write_rejected_outcome, _read_rejected_outcome)
"""Wire codec for ``RejectedOutcome`` (0 data words, 1 pointers)."""


def _write_cancelled_outcome(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["message"])


def _read_cancelled_outcome(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "message": s.get_text(0),
    }


_cancelled_outcome_codec = _Codec(0, 1, _write_cancelled_outcome, _read_cancelled_outcome)
"""Wire codec for ``CancelledOutcome`` (0 data words, 1 pointers)."""


def _write_suspended_outcome(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["checkpointJson"])
    s.set_u32(0, v["checkpointSchemaVersion"])
    s.set_u64(1, v["wakeAtUnixMs"])


def _read_suspended_outcome(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "checkpointJson": s.get_text(0),
        "checkpointSchemaVersion": s.get_u32(0),
        "wakeAtUnixMs": s.get_u64(1),
    }


_suspended_outcome_codec = _Codec(2, 1, _write_suspended_outcome, _read_suspended_outcome)
"""Wire codec for ``SuspendedOutcome`` (2 data words, 1 pointers)."""


def _write_job_outcome(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "completed":
        s.set_u16(0, 0)
        _completed_outcome_codec.write(s.init_struct(0, 1, 1), v["value"], caps)
    elif kind == "retryable":
        s.set_u16(0, 1)
        _retryable_outcome_codec.write(s.init_struct(0, 1, 1), v["value"], caps)
    elif kind == "rejected":
        s.set_u16(0, 2)
        _rejected_outcome_codec.write(s.init_struct(0, 0, 1), v["value"], caps)
    elif kind == "cancelled":
        s.set_u16(0, 3)
        _cancelled_outcome_codec.write(s.init_struct(0, 0, 1), v["value"], caps)
    elif kind == "suspended":
        s.set_u16(0, 4)
        _suspended_outcome_codec.write(s.init_struct(0, 2, 1), v["value"], caps)
    else:
        raise ValueError(f"unknown JobOutcome union member: {kind}")


def _read_job_outcome(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "completed", "value": _completed_outcome_codec.read(s.get_struct(0, 1, 1), caps)}
    elif disc == 1:
        return {"kind": "retryable", "value": _retryable_outcome_codec.read(s.get_struct(0, 1, 1), caps)}
    elif disc == 2:
        return {"kind": "rejected", "value": _rejected_outcome_codec.read(s.get_struct(0, 0, 1), caps)}
    elif disc == 3:
        return {"kind": "cancelled", "value": _cancelled_outcome_codec.read(s.get_struct(0, 0, 1), caps)}
    elif disc == 4:
        return {"kind": "suspended", "value": _suspended_outcome_codec.read(s.get_struct(0, 2, 1), caps)}
    raise ValueError(f"unknown JobOutcome union member: {disc}")


_job_outcome_codec = _Codec(1, 1, _write_job_outcome, _read_job_outcome)
"""Wire codec for ``JobOutcome`` (1 data words, 1 pointers)."""


def _write_domain_event(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["eventId"])
    s.set_text(1, v["eventType"])
    s.set_u32(0, v["schemaVersion"])
    s.set_u64(1, v["occurredAtUnixMs"])
    s.set_text(2, v["accountId"])
    s.set_text(3, v["correlationId"])
    s.set_text(4, v["causationId"])
    s.set_text(5, v["deduplicationKey"])
    s.set_u32(1, v["deliveryAttempt"])
    s.set_data(6, v["payload"])
    s.set_text(7, v["checkpointJson"])
    s.set_u32(4, v["checkpointSchemaVersion"])
    s.set_u32(5, v["invocationSequence"])
    s.set_bool(192, v["resumePending"])
    s.set_text(8, v["source"])


def _read_domain_event(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "eventId": s.get_text(0),
        "eventType": s.get_text(1),
        "schemaVersion": s.get_u32(0),
        "occurredAtUnixMs": s.get_u64(1),
        "accountId": s.get_text(2),
        "correlationId": s.get_text(3),
        "causationId": s.get_text(4),
        "deduplicationKey": s.get_text(5),
        "deliveryAttempt": s.get_u32(1),
        "payload": s.get_data(6),
        "checkpointJson": s.get_text(7),
        "checkpointSchemaVersion": s.get_u32(4),
        "invocationSequence": s.get_u32(5),
        "resumePending": s.get_bool(192),
        "source": s.get_text(8),
    }


_domain_event_codec = _Codec(4, 9, _write_domain_event, _read_domain_event)
"""Wire codec for ``DomainEvent`` (4 data words, 9 pointers)."""


def _write_event_ack(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_event_ack(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "dummy": None,
    }


_event_ack_codec = _Codec(0, 0, _write_event_ack, _read_event_ack)
"""Wire codec for ``EventAck`` (0 data words, 0 pointers)."""


def _write_event_retry(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_u64(0, v["retryAtUnixMs"])
    s.set_text(0, v["reason"])


def _read_event_retry(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "retryAtUnixMs": s.get_u64(0),
        "reason": s.get_text(0),
    }


_event_retry_codec = _Codec(1, 1, _write_event_retry, _read_event_retry)
"""Wire codec for ``EventRetry`` (1 data words, 1 pointers)."""


def _write_event_reject(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["reason"])


def _read_event_reject(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "reason": s.get_text(0),
    }


_event_reject_codec = _Codec(0, 1, _write_event_reject, _read_event_reject)
"""Wire codec for ``EventReject`` (0 data words, 1 pointers)."""


def _write_event_dead_letter(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["reason"])


def _read_event_dead_letter(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "reason": s.get_text(0),
    }


_event_dead_letter_codec = _Codec(0, 1, _write_event_dead_letter, _read_event_dead_letter)
"""Wire codec for ``EventDeadLetter`` (0 data words, 1 pointers)."""


def _write_event_suspended(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["checkpointJson"])
    s.set_u32(0, v["checkpointSchemaVersion"])
    s.set_u64(1, v["wakeAtUnixMs"])
    s.set_text(1, v["wakeOnEventType"])
    s.set_text(2, v["wakeOnFilterJson"])


def _read_event_suspended(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "checkpointJson": s.get_text(0),
        "checkpointSchemaVersion": s.get_u32(0),
        "wakeAtUnixMs": s.get_u64(1),
        "wakeOnEventType": s.get_text(1),
        "wakeOnFilterJson": s.get_text(2),
    }


_event_suspended_codec = _Codec(2, 3, _write_event_suspended, _read_event_suspended)
"""Wire codec for ``EventSuspended`` (2 data words, 3 pointers)."""


def _write_event_result(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ack":
        s.set_u16(0, 0)
        _event_ack_codec.write(s.init_struct(0, 0, 0), v["value"], caps)
    elif kind == "retry":
        s.set_u16(0, 1)
        _event_retry_codec.write(s.init_struct(0, 1, 1), v["value"], caps)
    elif kind == "reject":
        s.set_u16(0, 2)
        _event_reject_codec.write(s.init_struct(0, 0, 1), v["value"], caps)
    elif kind == "deadLetter":
        s.set_u16(0, 3)
        _event_dead_letter_codec.write(s.init_struct(0, 0, 1), v["value"], caps)
    elif kind == "suspended":
        s.set_u16(0, 4)
        _event_suspended_codec.write(s.init_struct(0, 2, 3), v["value"], caps)
    else:
        raise ValueError(f"unknown EventResult union member: {kind}")


def _read_event_result(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ack", "value": _event_ack_codec.read(s.get_struct(0, 0, 0), caps)}
    elif disc == 1:
        return {"kind": "retry", "value": _event_retry_codec.read(s.get_struct(0, 1, 1), caps)}
    elif disc == 2:
        return {"kind": "reject", "value": _event_reject_codec.read(s.get_struct(0, 0, 1), caps)}
    elif disc == 3:
        return {"kind": "deadLetter", "value": _event_dead_letter_codec.read(s.get_struct(0, 0, 1), caps)}
    elif disc == 4:
        return {"kind": "suspended", "value": _event_suspended_codec.read(s.get_struct(0, 2, 3), caps)}
    raise ValueError(f"unknown EventResult union member: {disc}")


_event_result_codec = _Codec(1, 1, _write_event_result, _read_event_result)
"""Wire codec for ``EventResult`` (1 data words, 1 pointers)."""


def _write_head_ok(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_bool(0, v["found"])
    _object_metadata_codec.write(s.init_struct(0, 1, 4), v["meta"], caps)


def _read_head_ok(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "found": s.get_bool(0),
        "meta": _object_metadata_codec.read(s.get_struct(0, 1, 4), caps),
    }


_head_ok_codec = _Codec(1, 1, _write_head_ok, _read_head_ok)
"""Wire codec for ``HeadOk`` (1 data words, 1 pointers)."""


def _write_head_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        _head_ok_codec.write(s.init_struct(0, 1, 1), v["value"], caps)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown HeadReply union member: {kind}")


def _read_head_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": _head_ok_codec.read(s.get_struct(0, 1, 1), caps)}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown HeadReply union member: {disc}")


_head_reply_codec = _Codec(1, 1, _write_head_reply, _read_head_reply)
"""Wire codec for ``HeadReply`` (1 data words, 1 pointers)."""


def _write_list_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        _list_page_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown ListReply union member: {kind}")


def _read_list_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": _list_page_codec.read(s.get_struct(0, 0, 2), caps)}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown ListReply union member: {disc}")


_list_reply_codec = _Codec(1, 1, _write_list_reply, _read_list_reply)
"""Wire codec for ``ListReply`` (1 data words, 1 pointers)."""


def _write_get_ok(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _object_metadata_codec.write(s.init_struct(0, 1, 4), v["meta"], caps)
    s.set_cap(1, caps.export_cap(v["body"]))


def _read_get_ok(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "meta": _object_metadata_codec.read(s.get_struct(0, 1, 4), caps),
        "body": caps.import_cap(s.get_cap_index(1)),
    }


_get_ok_codec = _Codec(0, 2, _write_get_ok, _read_get_ok)
"""Wire codec for ``GetOk`` (0 data words, 2 pointers)."""


def _write_get_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        _get_ok_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown GetReply union member: {kind}")


def _read_get_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": _get_ok_codec.read(s.get_struct(0, 0, 2), caps)}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown GetReply union member: {disc}")


_get_reply_codec = _Codec(1, 1, _write_get_reply, _read_get_reply)
"""Wire codec for ``GetReply`` (1 data words, 1 pointers)."""


def _write_put_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        _put_result_codec.write(s.init_struct(0, 1, 3), v["value"], caps)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown PutReply union member: {kind}")


def _read_put_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": _put_result_codec.read(s.get_struct(0, 1, 3), caps)}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown PutReply union member: {disc}")


_put_reply_codec = _Codec(1, 1, _write_put_reply, _read_put_reply)
"""Wire codec for ``PutReply`` (1 data words, 1 pointers)."""


def _write_copy_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        _copy_result_codec.write(s.init_struct(0, 1, 0), v["value"], caps)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown CopyReply union member: {kind}")


def _read_copy_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": _copy_result_codec.read(s.get_struct(0, 1, 0), caps)}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown CopyReply union member: {disc}")


_copy_reply_codec = _Codec(1, 1, _write_copy_reply, _read_copy_reply)
"""Wire codec for ``CopyReply`` (1 data words, 1 pointers)."""


def _write_empty_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown EmptyReply union member: {kind}")


def _read_empty_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok"}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown EmptyReply union member: {disc}")


_empty_reply_codec = _Codec(1, 1, _write_empty_reply, _read_empty_reply)
"""Wire codec for ``EmptyReply`` (1 data words, 1 pointers)."""


def _write_pull_ok(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_data(0, v["chunk"])
    s.set_bool(0, v["done"])


def _read_pull_ok(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "chunk": s.get_data(0),
        "done": s.get_bool(0),
    }


_pull_ok_codec = _Codec(1, 1, _write_pull_ok, _read_pull_ok)
"""Wire codec for ``PullOk`` (1 data words, 1 pointers)."""


def _write_pull_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        _pull_ok_codec.write(s.init_struct(0, 1, 1), v["value"], caps)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown PullReply union member: {kind}")


def _read_pull_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": _pull_ok_codec.read(s.get_struct(0, 1, 1), caps)}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown PullReply union member: {disc}")


_pull_reply_codec = _Codec(1, 1, _write_pull_reply, _read_pull_reply)
"""Wire codec for ``PullReply`` (1 data words, 1 pointers)."""


def _write_open_ok(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _object_metadata_codec.write(s.init_struct(0, 1, 4), v["meta"], caps)
    s.set_cap(1, caps.export_cap(v["body"]))


def _read_open_ok(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "meta": _object_metadata_codec.read(s.get_struct(0, 1, 4), caps),
        "body": caps.import_cap(s.get_cap_index(1)),
    }


_open_ok_codec = _Codec(0, 2, _write_open_ok, _read_open_ok)
"""Wire codec for ``OpenOk`` (0 data words, 2 pointers)."""


def _write_open_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        _open_ok_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown OpenReply union member: {kind}")


def _read_open_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": _open_ok_codec.read(s.get_struct(0, 0, 2), caps)}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown OpenReply union member: {disc}")


_open_reply_codec = _Codec(1, 1, _write_open_reply, _read_open_reply)
"""Wire codec for ``OpenReply`` (1 data words, 1 pointers)."""


def _write_describe_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        _plugin_describe_codec.write(s.init_struct(0, 1, 7), v["value"], caps)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown DescribeReply union member: {kind}")


def _read_describe_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": _plugin_describe_codec.read(s.get_struct(0, 1, 7), caps)}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown DescribeReply union member: {disc}")


_describe_reply_codec = _Codec(1, 1, _write_describe_reply, _read_describe_reply)
"""Wire codec for ``DescribeReply`` (1 data words, 1 pointers)."""


def _write_destination_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        s.set_cap(0, caps.export_cap(v["value"]))
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown DestinationReply union member: {kind}")


def _read_destination_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": caps.import_cap(s.get_cap_index(0))}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown DestinationReply union member: {disc}")


_destination_reply_codec = _Codec(1, 1, _write_destination_reply, _read_destination_reply)
"""Wire codec for ``DestinationReply`` (1 data words, 1 pointers)."""


def _write_source_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        s.set_cap(0, caps.export_cap(v["value"]))
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown SourceReply union member: {kind}")


def _read_source_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": caps.import_cap(s.get_cap_index(0))}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown SourceReply union member: {disc}")


_source_reply_codec = _Codec(1, 1, _write_source_reply, _read_source_reply)
"""Wire codec for ``SourceReply`` (1 data words, 1 pointers)."""


def _write_worker_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        s.set_cap(0, caps.export_cap(v["value"]))
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown WorkerReply union member: {kind}")


def _read_worker_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": caps.import_cap(s.get_cap_index(0))}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown WorkerReply union member: {disc}")


_worker_reply_codec = _Codec(1, 1, _write_worker_reply, _read_worker_reply)
"""Wire codec for ``WorkerReply`` (1 data words, 1 pointers)."""


def _write_handle_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        _job_outcome_codec.write(s.init_struct(0, 1, 1), v["value"], caps)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown HandleReply union member: {kind}")


def _read_handle_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": _job_outcome_codec.read(s.get_struct(0, 1, 1), caps)}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown HandleReply union member: {disc}")


_handle_reply_codec = _Codec(1, 1, _write_handle_reply, _read_handle_reply)
"""Wire codec for ``HandleReply`` (1 data words, 1 pointers)."""


def _write_content_source_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        s.set_cap(0, caps.export_cap(v["value"]))
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown ContentSourceReply union member: {kind}")


def _read_content_source_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": caps.import_cap(s.get_cap_index(0))}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown ContentSourceReply union member: {disc}")


_content_source_reply_codec = _Codec(1, 1, _write_content_source_reply, _read_content_source_reply)
"""Wire codec for ``ContentSourceReply`` (1 data words, 1 pointers)."""


def _write_integration_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        s.set_cap(0, caps.export_cap(v["value"]))
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown IntegrationReply union member: {kind}")


def _read_integration_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": caps.import_cap(s.get_cap_index(0))}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown IntegrationReply union member: {disc}")


_integration_reply_codec = _Codec(1, 1, _write_integration_reply, _read_integration_reply)
"""Wire codec for ``IntegrationReply`` (1 data words, 1 pointers)."""


def _write_database_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        s.set_cap(0, caps.export_cap(v["value"]))
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown DatabaseReply union member: {kind}")


def _read_database_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": caps.import_cap(s.get_cap_index(0))}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown DatabaseReply union member: {disc}")


_database_reply_codec = _Codec(1, 1, _write_database_reply, _read_database_reply)
"""Wire codec for ``DatabaseReply`` (1 data words, 1 pointers)."""


def _write_event_result_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        _event_result_codec.write(s.init_struct(0, 1, 1), v["value"], caps)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown EventResultReply union member: {kind}")


def _read_event_result_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": _event_result_codec.read(s.get_struct(0, 1, 1), caps)}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown EventResultReply union member: {disc}")


_event_result_reply_codec = _Codec(1, 1, _write_event_result_reply, _read_event_result_reply)
"""Wire codec for ``EventResultReply`` (1 data words, 1 pointers)."""


def _write_json_ok(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["json"])


def _read_json_ok(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "json": s.get_text(0),
    }


_json_ok_codec = _Codec(0, 1, _write_json_ok, _read_json_ok)
"""Wire codec for ``JsonOk`` (0 data words, 1 pointers)."""


def _write_json_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        _json_ok_codec.write(s.init_struct(0, 0, 1), v["value"], caps)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown JsonReply union member: {kind}")


def _read_json_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": _json_ok_codec.read(s.get_struct(0, 0, 1), caps)}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown JsonReply union member: {disc}")


_json_reply_codec = _Codec(1, 1, _write_json_reply, _read_json_reply)
"""Wire codec for ``JsonReply`` (1 data words, 1 pointers)."""


def _write_health_ok(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_bool(0, v["ok"])
    s.set_text(0, v["detail"])


def _read_health_ok(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "ok": s.get_bool(0),
        "detail": s.get_text(0),
    }


_health_ok_codec = _Codec(1, 1, _write_health_ok, _read_health_ok)
"""Wire codec for ``HealthOk`` (1 data words, 1 pointers)."""


def _write_health_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        _health_ok_codec.write(s.init_struct(0, 1, 1), v["value"], caps)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown HealthReply union member: {kind}")


def _read_health_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": _health_ok_codec.read(s.get_struct(0, 1, 1), caps)}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown HealthReply union member: {disc}")


_health_reply_codec = _Codec(1, 1, _write_health_reply, _read_health_reply)
"""Wire codec for ``HealthReply`` (1 data words, 1 pointers)."""


def _write_adapter_session_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        s.set_cap(0, caps.export_cap(v["value"]))
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown AdapterSessionReply union member: {kind}")


def _read_adapter_session_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": caps.import_cap(s.get_cap_index(0))}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown AdapterSessionReply union member: {disc}")


_adapter_session_reply_codec = _Codec(1, 1, _write_adapter_session_reply, _read_adapter_session_reply)
"""Wire codec for ``AdapterSessionReply`` (1 data words, 1 pointers)."""


def _write_guest_database_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        s.set_cap(0, caps.export_cap(v["value"]))
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown GuestDatabaseReply union member: {kind}")


def _read_guest_database_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": caps.import_cap(s.get_cap_index(0))}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown GuestDatabaseReply union member: {disc}")


_guest_database_reply_codec = _Codec(1, 1, _write_guest_database_reply, _read_guest_database_reply)
"""Wire codec for ``GuestDatabaseReply`` (1 data words, 1 pointers)."""


def _write_named_database(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["name"])
    s.set_cap(1, caps.export_cap(v["database"]))


def _read_named_database(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "name": s.get_text(0),
        "database": caps.import_cap(s.get_cap_index(1)),
    }


_named_database_codec = _Codec(0, 2, _write_named_database, _read_named_database)
"""Wire codec for ``NamedDatabase`` (0 data words, 2 pointers)."""


def _write_db_value(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "null":
        s.set_u16(1, 0)
        s.set_u16(0, _ord(A.DB_TYPES, v["value"], "DbType"))
    elif kind == "boolean":
        s.set_u16(1, 1)
        s.set_bool(0, v["value"])
    elif kind == "int64":
        s.set_u16(1, 2)
        s.set_i64(1, v["value"])
    elif kind == "float64":
        s.set_u16(1, 3)
        s.set_f64(1, v["value"])
    elif kind == "text":
        s.set_u16(1, 4)
        s.set_text(0, v["value"])
    elif kind == "bytes":
        s.set_u16(1, 5)
        s.set_data(0, v["value"])
    else:
        raise ValueError(f"unknown DbValue union member: {kind}")


def _read_db_value(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(1)
    if disc == 0:
        return {"kind": "null", "value": _from_ord(A.DB_TYPES, s.get_u16(0), "DbType")}
    elif disc == 1:
        return {"kind": "boolean", "value": s.get_bool(0)}
    elif disc == 2:
        return {"kind": "int64", "value": s.get_i64(1)}
    elif disc == 3:
        return {"kind": "float64", "value": s.get_f64(1)}
    elif disc == 4:
        return {"kind": "text", "value": s.get_text(0)}
    elif disc == 5:
        return {"kind": "bytes", "value": s.get_data(0)}
    raise ValueError(f"unknown DbValue union member: {disc}")


_db_value_codec = _Codec(2, 1, _write_db_value, _read_db_value)
"""Wire codec for ``DbValue`` (2 data words, 1 pointers)."""


def _write_db_column(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["name"])
    s.set_u16(0, _ord(A.DB_TYPES, v["dbType"], "DbType"))


def _read_db_column(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "name": s.get_text(0),
        "dbType": _from_ord(A.DB_TYPES, s.get_u16(0), "DbType"),
    }


_db_column_codec = _Codec(1, 1, _write_db_column, _read_db_column)
"""Wire codec for ``DbColumn`` (1 data words, 1 pointers)."""


def _write_db_row(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    items = s.init_struct_list(0, len(v["values"]), 2, 1)
    for item, elem in zip(items, v["values"], strict=True):
        _db_value_codec.write(item, elem, caps)


def _read_db_row(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "values": [_db_value_codec.read(item, caps) for item in s.get_struct_list(0, 2, 1)],
    }


_db_row_codec = _Codec(0, 1, _write_db_row, _read_db_row)
"""Wire codec for ``DbRow`` (0 data words, 1 pointers)."""


def _write_db_statement(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["sql"])
    items = s.init_struct_list(1, len(v["parameters"]), 2, 1)
    for item, elem in zip(items, v["parameters"], strict=True):
        _db_value_codec.write(item, elem, caps)
    s.set_u16(0, _ord(A.DB_STATEMENT_KINDS, v["kind"], "DbStatementKind"))
    s.set_u32(1, v["maxRows"])
    s.set_u16(1, _ord(A.DB_RESULT_SELECTIONS, v["resultSelection"], "DbResultSelection"))


def _read_db_statement(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "sql": s.get_text(0),
        "parameters": [_db_value_codec.read(item, caps) for item in s.get_struct_list(1, 2, 1)],
        "kind": _from_ord(A.DB_STATEMENT_KINDS, s.get_u16(0), "DbStatementKind"),
        "maxRows": s.get_u32(1),
        "resultSelection": _from_ord(A.DB_RESULT_SELECTIONS, s.get_u16(1), "DbResultSelection"),
    }


_db_statement_codec = _Codec(1, 2, _write_db_statement, _read_db_statement)
"""Wire codec for ``DbStatement`` (1 data words, 2 pointers)."""


def _write_execute_request(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["operationId"])
    s.set_text(1, v["requestHash"])
    items = s.init_struct_list(2, len(v["statements"]), 1, 2)
    for item, elem in zip(items, v["statements"], strict=True):
        _db_statement_codec.write(item, elem, caps)
    s.set_u64(0, v["deadlineUnixMs"])


def _read_execute_request(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "operationId": s.get_text(0),
        "requestHash": s.get_text(1),
        "statements": [_db_statement_codec.read(item, caps) for item in s.get_struct_list(2, 1, 2)],
        "deadlineUnixMs": s.get_u64(0),
    }


_execute_request_codec = _Codec(1, 3, _write_execute_request, _read_execute_request)
"""Wire codec for ``ExecuteRequest`` (1 data words, 3 pointers)."""


def _write_sql_span(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_u32(0, v["start"])
    s.set_u32(1, v["end"])


def _read_sql_span(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "start": s.get_u32(0),
        "end": s.get_u32(1),
    }


_sql_span_codec = _Codec(1, 0, _write_sql_span, _read_sql_span)
"""Wire codec for ``SqlSpan`` (1 data words, 0 pointers)."""


def _write_text_collate_site(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _sql_span_codec.write(s.init_struct(0, 1, 0), v["span"], caps)


def _read_text_collate_site(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "span": _sql_span_codec.read(s.get_struct(0, 1, 0), caps),
    }


_text_collate_site_codec = _Codec(0, 1, _write_text_collate_site, _read_text_collate_site)
"""Wire codec for ``TextCollateSite`` (0 data words, 1 pointers)."""


def _write_integer_arith_site(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _sql_span_codec.write(s.init_struct(0, 1, 0), v["full"], caps)
    _sql_span_codec.write(s.init_struct(1, 1, 0), v["lhs"], caps)
    _sql_span_codec.write(s.init_struct(2, 1, 0), v["rhs"], caps)
    s.set_u16(0, _ord(A.INTEGER_ARITH_KINDS, v["kind"], "IntegerArithKind"))


def _read_integer_arith_site(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "full": _sql_span_codec.read(s.get_struct(0, 1, 0), caps),
        "lhs": _sql_span_codec.read(s.get_struct(1, 1, 0), caps),
        "rhs": _sql_span_codec.read(s.get_struct(2, 1, 0), caps),
        "kind": _from_ord(A.INTEGER_ARITH_KINDS, s.get_u16(0), "IntegerArithKind"),
    }


_integer_arith_site_codec = _Codec(1, 3, _write_integer_arith_site, _read_integer_arith_site)
"""Wire codec for ``IntegerArithSite`` (1 data words, 3 pointers)."""


def _write_physical_access(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["table"])
    s.set_text(1, v["column"])


def _read_physical_access(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "table": s.get_text(0),
        "column": s.get_text(1),
    }


_physical_access_codec = _Codec(0, 2, _write_physical_access, _read_physical_access)
"""Wire codec for ``PhysicalAccess`` (0 data words, 2 pointers)."""


def _write_resolved_assignment(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["table"])
    s.set_text(1, v["column"])
    s.set_u16(0, _ord(A.RESOLVED_SQL_TYPES, v["dest"], "ResolvedSqlType"))
    s.set_u16(1, _ord(A.RESOLVED_SQL_TYPES, v["source"], "ResolvedSqlType"))


def _read_resolved_assignment(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "table": s.get_text(0),
        "column": s.get_text(1),
        "dest": _from_ord(A.RESOLVED_SQL_TYPES, s.get_u16(0), "ResolvedSqlType"),
        "source": _from_ord(A.RESOLVED_SQL_TYPES, s.get_u16(1), "ResolvedSqlType"),
    }


_resolved_assignment_codec = _Codec(1, 2, _write_resolved_assignment, _read_resolved_assignment)
"""Wire codec for ``ResolvedAssignment`` (1 data words, 2 pointers)."""


def _write_named_sql_type(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["name"])
    s.set_u16(0, _ord(A.RESOLVED_SQL_TYPES, v["sqlType"], "ResolvedSqlType"))


def _read_named_sql_type(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "name": s.get_text(0),
        "sqlType": _from_ord(A.RESOLVED_SQL_TYPES, s.get_u16(0), "ResolvedSqlType"),
    }


_named_sql_type_codec = _Codec(1, 1, _write_named_sql_type, _read_named_sql_type)
"""Wire codec for ``NamedSqlType`` (1 data words, 1 pointers)."""


def _write_column_reference(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["refTable"])
    s.set_text_list(1, v["refColumns"])


def _read_column_reference(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "refTable": s.get_text(0),
        "refColumns": s.get_text_list(1),
    }


_column_reference_codec = _Codec(0, 2, _write_column_reference, _read_column_reference)
"""Wire codec for ``ColumnReference`` (0 data words, 2 pointers)."""


def _write_optional_column_reference(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "none":
        s.set_u16(0, 0)
    elif kind == "some":
        s.set_u16(0, 1)
        _column_reference_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown OptionalColumnReference union member: {kind}")


def _read_optional_column_reference(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "none"}
    elif disc == 1:
        return {"kind": "some", "value": _column_reference_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown OptionalColumnReference union member: {disc}")


_optional_column_reference_codec = _Codec(1, 1, _write_optional_column_reference, _read_optional_column_reference)
"""Wire codec for ``OptionalColumnReference`` (1 data words, 1 pointers)."""


def _write_foreign_key_constraint(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text_list(0, v["columns"])
    s.set_text(1, v["refTable"])
    s.set_text_list(2, v["refColumns"])


def _read_foreign_key_constraint(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "columns": s.get_text_list(0),
        "refTable": s.get_text(1),
        "refColumns": s.get_text_list(2),
    }


_foreign_key_constraint_codec = _Codec(0, 3, _write_foreign_key_constraint, _read_foreign_key_constraint)
"""Wire codec for ``ForeignKeyConstraint`` (0 data words, 3 pointers)."""


def _write_table_constraint(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "primaryKey":
        s.set_u16(0, 0)
        s.set_text_list(0, v["value"])
    elif kind == "unique":
        s.set_u16(0, 1)
        s.set_text_list(0, v["value"])
    elif kind == "check":
        s.set_u16(0, 2)
        s.set_text(0, v["value"])
    elif kind == "foreignKey":
        s.set_u16(0, 3)
        _foreign_key_constraint_codec.write(s.init_struct(0, 0, 3), v["value"], caps)
    else:
        raise ValueError(f"unknown TableConstraint union member: {kind}")


def _read_table_constraint(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "primaryKey", "value": s.get_text_list(0)}
    elif disc == 1:
        return {"kind": "unique", "value": s.get_text_list(0)}
    elif disc == 2:
        return {"kind": "check", "value": s.get_text(0)}
    elif disc == 3:
        return {"kind": "foreignKey", "value": _foreign_key_constraint_codec.read(s.get_struct(0, 0, 3), caps)}
    raise ValueError(f"unknown TableConstraint union member: {disc}")


_table_constraint_codec = _Codec(1, 1, _write_table_constraint, _read_table_constraint)
"""Wire codec for ``TableConstraint`` (1 data words, 1 pointers)."""


def _write_create_table_schema(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["table"])
    items = s.init_struct_list(1, len(v["columns"]), 1, 1)
    for item, elem in zip(items, v["columns"], strict=True):
        _named_sql_type_codec.write(item, elem, caps)
    s.set_text(2, v["identityColumn"])
    s.set_bool_list(3, v["columnNotNull"])
    s.set_bool_list(4, v["columnUnique"])
    s.set_bool_list(5, v["columnPrimaryKey"])
    s.set_text_list(6, v["columnDefaults"])
    s.set_text_list(7, v["columnChecks"])
    items = s.init_struct_list(8, len(v["columnReferences"]), 1, 1)
    for item, elem in zip(items, v["columnReferences"], strict=True):
        _optional_column_reference_codec.write(item, elem, caps)
    items = s.init_struct_list(9, len(v["tableConstraints"]), 1, 1)
    for item, elem in zip(items, v["tableConstraints"], strict=True):
        _table_constraint_codec.write(item, elem, caps)


def _read_create_table_schema(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "table": s.get_text(0),
        "columns": [_named_sql_type_codec.read(item, caps) for item in s.get_struct_list(1, 1, 1)],
        "identityColumn": s.get_text(2),
        "columnNotNull": s.get_bool_list(3),
        "columnUnique": s.get_bool_list(4),
        "columnPrimaryKey": s.get_bool_list(5),
        "columnDefaults": s.get_text_list(6),
        "columnChecks": s.get_text_list(7),
        "columnReferences": [_optional_column_reference_codec.read(item, caps) for item in s.get_struct_list(8, 1, 1)],
        "tableConstraints": [_table_constraint_codec.read(item, caps) for item in s.get_struct_list(9, 1, 1)],
    }


_create_table_schema_codec = _Codec(0, 10, _write_create_table_schema, _read_create_table_schema)
"""Wire codec for ``CreateTableSchema`` (0 data words, 10 pointers)."""


def _write_schema_create(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _create_table_schema_codec.write(s.init_struct(0, 0, 10), v["schema"], caps)
    s.set_text(1, v["fingerprint"])
    s.set_bool(0, v["noop"])


def _read_schema_create(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "schema": _create_table_schema_codec.read(s.get_struct(0, 0, 10), caps),
        "fingerprint": s.get_text(1),
        "noop": s.get_bool(0),
    }


_schema_create_codec = _Codec(1, 2, _write_schema_create, _read_schema_create)
"""Wire codec for ``SchemaCreate`` (1 data words, 2 pointers)."""


def _write_schema_action(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "none":
        s.set_u16(0, 0)
    elif kind == "create":
        s.set_u16(0, 1)
        _schema_create_codec.write(s.init_struct(0, 1, 2), v["value"], caps)
    elif kind == "drop":
        s.set_u16(0, 2)
        s.set_text(0, v["value"])
    else:
        raise ValueError(f"unknown SchemaAction union member: {kind}")


def _read_schema_action(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "none"}
    elif disc == 1:
        return {"kind": "create", "value": _schema_create_codec.read(s.get_struct(0, 1, 2), caps)}
    elif disc == 2:
        return {"kind": "drop", "value": s.get_text(0)}
    raise ValueError(f"unknown SchemaAction union member: {disc}")


_schema_action_codec = _Codec(1, 1, _write_schema_action, _read_schema_action)
"""Wire codec for ``SchemaAction`` (1 data words, 1 pointers)."""


def _write_resolved_statement(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["statementHash"])
    items = s.init_struct_list(1, len(v["outputColumns"]), 1, 1)
    for item, elem in zip(items, v["outputColumns"], strict=True):
        _named_sql_type_codec.write(item, elem, caps)
    items = s.init_struct_list(2, len(v["physicalAccesses"]), 0, 2)
    for item, elem in zip(items, v["physicalAccesses"], strict=True):
        _physical_access_codec.write(item, elem, caps)
    items = s.init_struct_list(3, len(v["assignments"]), 1, 2)
    for item, elem in zip(items, v["assignments"], strict=True):
        _resolved_assignment_codec.write(item, elem, caps)
    items = s.init_struct_list(4, len(v["textCollateSites"]), 0, 1)
    for item, elem in zip(items, v["textCollateSites"], strict=True):
        _text_collate_site_codec.write(item, elem, caps)
    items = s.init_struct_list(5, len(v["integerArithSites"]), 1, 3)
    for item, elem in zip(items, v["integerArithSites"], strict=True):
        _integer_arith_site_codec.write(item, elem, caps)
    s.set_text_list(6, v["functions"])
    _schema_action_codec.write(s.init_struct(7, 1, 1), v["schemaAction"], caps)


def _read_resolved_statement(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "statementHash": s.get_text(0),
        "outputColumns": [_named_sql_type_codec.read(item, caps) for item in s.get_struct_list(1, 1, 1)],
        "physicalAccesses": [_physical_access_codec.read(item, caps) for item in s.get_struct_list(2, 0, 2)],
        "assignments": [_resolved_assignment_codec.read(item, caps) for item in s.get_struct_list(3, 1, 2)],
        "textCollateSites": [_text_collate_site_codec.read(item, caps) for item in s.get_struct_list(4, 0, 1)],
        "integerArithSites": [_integer_arith_site_codec.read(item, caps) for item in s.get_struct_list(5, 1, 3)],
        "functions": s.get_text_list(6),
        "schemaAction": _schema_action_codec.read(s.get_struct(7, 1, 1), caps),
    }


_resolved_statement_codec = _Codec(0, 8, _write_resolved_statement, _read_resolved_statement)
"""Wire codec for ``ResolvedStatement`` (0 data words, 8 pointers)."""


def _write_adapter_receipt(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_u32(0, v["guestLen"])
    s.set_text(0, v["guestHash"])


def _read_adapter_receipt(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "guestLen": s.get_u32(0),
        "guestHash": s.get_text(0),
    }


_adapter_receipt_codec = _Codec(1, 1, _write_adapter_receipt, _read_adapter_receipt)
"""Wire codec for ``AdapterReceipt`` (1 data words, 1 pointers)."""


def _write_adapter_statement(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["sql"])
    items = s.init_struct_list(1, len(v["parameters"]), 2, 1)
    for item, elem in zip(items, v["parameters"], strict=True):
        _db_value_codec.write(item, elem, caps)
    s.set_u16(0, _ord(A.DB_STATEMENT_KINDS, v["kind"], "DbStatementKind"))
    s.set_u32(1, v["maxRows"])
    s.set_u16(1, _ord(A.DB_RESULT_SELECTIONS, v["resultSelection"], "DbResultSelection"))
    _resolved_statement_codec.write(s.init_struct(2, 0, 8), v["proof"], caps)


def _read_adapter_statement(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "sql": s.get_text(0),
        "parameters": [_db_value_codec.read(item, caps) for item in s.get_struct_list(1, 2, 1)],
        "kind": _from_ord(A.DB_STATEMENT_KINDS, s.get_u16(0), "DbStatementKind"),
        "maxRows": s.get_u32(1),
        "resultSelection": _from_ord(A.DB_RESULT_SELECTIONS, s.get_u16(1), "DbResultSelection"),
        "proof": _resolved_statement_codec.read(s.get_struct(2, 0, 8), caps),
    }


_adapter_statement_codec = _Codec(1, 3, _write_adapter_statement, _read_adapter_statement)
"""Wire codec for ``AdapterStatement`` (1 data words, 3 pointers)."""


def _write_adapter_execute_request(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["operationId"])
    s.set_text(1, v["requestHash"])
    items = s.init_struct_list(2, len(v["statements"]), 1, 3)
    for item, elem in zip(items, v["statements"], strict=True):
        _adapter_statement_codec.write(item, elem, caps)
    s.set_u64(0, v["deadlineUnixMs"])
    s.set_u16(4, _ord(A.ISOLATION_REQS, v["isolation"], "IsolationReq"))
    _adapter_receipt_codec.write(s.init_struct(3, 1, 1), v["receipt"], caps)


def _read_adapter_execute_request(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "operationId": s.get_text(0),
        "requestHash": s.get_text(1),
        "statements": [_adapter_statement_codec.read(item, caps) for item in s.get_struct_list(2, 1, 3)],
        "deadlineUnixMs": s.get_u64(0),
        "isolation": _from_ord(A.ISOLATION_REQS, s.get_u16(4), "IsolationReq"),
        "receipt": _adapter_receipt_codec.read(s.get_struct(3, 1, 1), caps),
    }


_adapter_execute_request_codec = _Codec(2, 4, _write_adapter_execute_request, _read_adapter_execute_request)
"""Wire codec for ``AdapterExecuteRequest`` (2 data words, 4 pointers)."""


def _write_statement_result(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    items = s.init_struct_list(0, len(v["rows"]), 0, 1)
    for item, elem in zip(items, v["rows"], strict=True):
        _db_row_codec.write(item, elem, caps)
    items = s.init_struct_list(1, len(v["columns"]), 1, 1)
    for item, elem in zip(items, v["columns"], strict=True):
        _db_column_codec.write(item, elem, caps)
    s.set_u64(0, v["rowsAffected"])


def _read_statement_result(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "rows": [_db_row_codec.read(item, caps) for item in s.get_struct_list(0, 0, 1)],
        "columns": [_db_column_codec.read(item, caps) for item in s.get_struct_list(1, 1, 1)],
        "rowsAffected": s.get_u64(0),
    }


_statement_result_codec = _Codec(1, 2, _write_statement_result, _read_statement_result)
"""Wire codec for ``StatementResult`` (1 data words, 2 pointers)."""


def _write_db_timing(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_u64(0, v["attemptElapsedUs"])
    s.set_u64(1, v["dbExecutionUs"])
    s.set_text(0, v["dbTimingSource"])


def _read_db_timing(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "attemptElapsedUs": s.get_u64(0),
        "dbExecutionUs": s.get_u64(1),
        "dbTimingSource": s.get_text(0),
    }


_db_timing_codec = _Codec(2, 1, _write_db_timing, _read_db_timing)
"""Wire codec for ``DbTiming`` (2 data words, 1 pointers)."""


def _write_execute_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["operationId"])
    items = s.init_struct_list(1, len(v["statements"]), 1, 2)
    for item, elem in zip(items, v["statements"], strict=True):
        _statement_result_codec.write(item, elem, caps)
    _db_timing_codec.write(s.init_struct(2, 2, 1), v["timing"], caps)


def _read_execute_reply(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "operationId": s.get_text(0),
        "statements": [_statement_result_codec.read(item, caps) for item in s.get_struct_list(1, 1, 2)],
        "timing": _db_timing_codec.read(s.get_struct(2, 2, 1), caps),
    }


_execute_reply_codec = _Codec(0, 3, _write_execute_reply, _read_execute_reply)
"""Wire codec for ``ExecuteReply`` (0 data words, 3 pointers)."""


def _write_execute_result_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        _execute_reply_codec.write(s.init_struct(0, 0, 3), v["value"], caps)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown ExecuteResultReply union member: {kind}")


def _read_execute_result_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": _execute_reply_codec.read(s.get_struct(0, 0, 3), caps)}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown ExecuteResultReply union member: {disc}")


_execute_result_reply_codec = _Codec(1, 1, _write_execute_result_reply, _read_execute_result_reply)
"""Wire codec for ``ExecuteResultReply`` (1 data words, 1 pointers)."""


def _write_db_capabilities(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_u32(0, v["sqlContractVersion"])
    s.set_bool(32, v["atomicBatch"])
    s.set_bool(33, v["returning"])
    s.set_bool(34, v["affectedRows"])
    s.set_bool(35, v["schemaMigrations"])
    s.set_bool(36, v["cancellation"])
    s.set_bool(37, v["timing"])
    s.set_u32(2, v["maxBinds"])
    s.set_u32(3, v["maxStatements"])
    s.set_u32(4, v["maxResultRows"])
    s.set_u32(5, v["maxPayloadBytes"])
    s.set_u32(6, v["maxResultBytes"])
    s.set_u32(7, v["maxCellBytes"])
    s.set_u32(8, v["maxRequestBytes"])
    s.set_u32(9, v["maxAtomicResultBytes"])
    s.set_bool(38, v["pluginDatabases"])
    s.set_u32(10, v["maxFunctionArgs"])
    s.set_u32(11, v["maxSchemaColumns"])
    s.set_u32(12, v["maxPatternBytes"])
    s.set_u32(13, v["maxLoweredStatementBytes"])
    s.set_bool(39, v["consistentBackupRead"])
    s.set_bool(40, v["atomicUnitRestore"])


def _read_db_capabilities(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "sqlContractVersion": s.get_u32(0),
        "atomicBatch": s.get_bool(32),
        "returning": s.get_bool(33),
        "affectedRows": s.get_bool(34),
        "schemaMigrations": s.get_bool(35),
        "cancellation": s.get_bool(36),
        "timing": s.get_bool(37),
        "maxBinds": s.get_u32(2),
        "maxStatements": s.get_u32(3),
        "maxResultRows": s.get_u32(4),
        "maxPayloadBytes": s.get_u32(5),
        "maxResultBytes": s.get_u32(6),
        "maxCellBytes": s.get_u32(7),
        "maxRequestBytes": s.get_u32(8),
        "maxAtomicResultBytes": s.get_u32(9),
        "pluginDatabases": s.get_bool(38),
        "maxFunctionArgs": s.get_u32(10),
        "maxSchemaColumns": s.get_u32(11),
        "maxPatternBytes": s.get_u32(12),
        "maxLoweredStatementBytes": s.get_u32(13),
        "consistentBackupRead": s.get_bool(39),
        "atomicUnitRestore": s.get_bool(40),
    }


_db_capabilities_codec = _Codec(7, 0, _write_db_capabilities, _read_db_capabilities)
"""Wire codec for ``DbCapabilities`` (7 data words, 0 pointers)."""


def _write_db_bootstrap_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        _db_bootstrap_codec.write(s.init_struct(0, 0, 1), v["value"], caps)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown DbBootstrapReply union member: {kind}")


def _read_db_bootstrap_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": _db_bootstrap_codec.read(s.get_struct(0, 0, 1), caps)}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown DbBootstrapReply union member: {disc}")


_db_bootstrap_reply_codec = _Codec(1, 1, _write_db_bootstrap_reply, _read_db_bootstrap_reply)
"""Wire codec for ``DbBootstrapReply`` (1 data words, 1 pointers)."""


def _write_db_bootstrap(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["engine"])


def _read_db_bootstrap(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "engine": s.get_text(0),
    }


_db_bootstrap_codec = _Codec(0, 1, _write_db_bootstrap, _read_db_bootstrap)
"""Wire codec for ``DbBootstrap`` (0 data words, 1 pointers)."""


def _write_db_capabilities_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        _db_capabilities_codec.write(s.init_struct(0, 7, 0), v["value"], caps)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown DbCapabilitiesReply union member: {kind}")


def _read_db_capabilities_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": _db_capabilities_codec.read(s.get_struct(0, 7, 0), caps)}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown DbCapabilitiesReply union member: {disc}")


_db_capabilities_reply_codec = _Codec(1, 1, _write_db_capabilities_reply, _read_db_capabilities_reply)
"""Wire codec for ``DbCapabilitiesReply`` (1 data words, 1 pointers)."""


def _write_identity_high_water(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["table"])
    s.set_i64(0, v["last"])


def _read_identity_high_water(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "table": s.get_text(0),
        "last": s.get_i64(0),
    }


_identity_high_water_codec = _Codec(1, 1, _write_identity_high_water, _read_identity_high_water)
"""Wire codec for ``IdentityHighWater`` (1 data words, 1 pointers)."""


def _write_identity_export_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        items = s.init_struct_list(0, len(v["value"]), 1, 1)
        for item, elem in zip(items, v["value"], strict=True):
            _identity_high_water_codec.write(item, elem, caps)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown IdentityExportReply union member: {kind}")


def _read_identity_export_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": [_identity_high_water_codec.read(item, caps) for item in s.get_struct_list(0, 1, 1)]}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown IdentityExportReply union member: {disc}")


_identity_export_reply_codec = _Codec(1, 1, _write_identity_export_reply, _read_identity_export_reply)
"""Wire codec for ``IdentityExportReply`` (1 data words, 1 pointers)."""


def _write_user_relations_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        s.set_text_list(0, v["value"])
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown UserRelationsReply union member: {kind}")


def _read_user_relations_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": s.get_text_list(0)}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown UserRelationsReply union member: {disc}")


_user_relations_reply_codec = _Codec(1, 1, _write_user_relations_reply, _read_user_relations_reply)
"""Wire codec for ``UserRelationsReply`` (1 data words, 1 pointers)."""


def _write_plugin_migration_op(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "schema":
        s.set_u16(0, 0)
        s.set_text(0, v["value"])
    elif kind == "data":
        s.set_u16(0, 1)
        s.set_text(0, v["value"])
    else:
        raise ValueError(f"unknown PluginMigrationOp union member: {kind}")


def _read_plugin_migration_op(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "schema", "value": s.get_text(0)}
    elif disc == 1:
        return {"kind": "data", "value": s.get_text(0)}
    raise ValueError(f"unknown PluginMigrationOp union member: {disc}")


_plugin_migration_op_codec = _Codec(1, 1, _write_plugin_migration_op, _read_plugin_migration_op)
"""Wire codec for ``PluginMigrationOp`` (1 data words, 1 pointers)."""


def _write_plugin_migration(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["id"])
    items = s.init_struct_list(1, len(v["operations"]), 1, 1)
    for item, elem in zip(items, v["operations"], strict=True):
        _plugin_migration_op_codec.write(item, elem, caps)


def _read_plugin_migration(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "id": s.get_text(0),
        "operations": [_plugin_migration_op_codec.read(item, caps) for item in s.get_struct_list(1, 1, 1)],
    }


_plugin_migration_codec = _Codec(0, 2, _write_plugin_migration, _read_plugin_migration)
"""Wire codec for ``PluginMigration`` (0 data words, 2 pointers)."""


def _write_plugin_migrations_ok(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    items = s.init_struct_list(0, len(v["migrations"]), 0, 2)
    for item, elem in zip(items, v["migrations"], strict=True):
        _plugin_migration_codec.write(item, elem, caps)


def _read_plugin_migrations_ok(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "migrations": [_plugin_migration_codec.read(item, caps) for item in s.get_struct_list(0, 0, 2)],
    }


_plugin_migrations_ok_codec = _Codec(0, 1, _write_plugin_migrations_ok, _read_plugin_migrations_ok)
"""Wire codec for ``PluginMigrationsOk`` (0 data words, 1 pointers)."""


def _write_plugin_migrations_reply(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    kind = v["kind"]
    if kind == "ok":
        s.set_u16(0, 0)
        _plugin_migrations_ok_codec.write(s.init_struct(0, 0, 1), v["value"], caps)
    elif kind == "err":
        s.set_u16(0, 1)
        _plugin_error_codec.write(s.init_struct(0, 0, 2), v["value"], caps)
    else:
        raise ValueError(f"unknown PluginMigrationsReply union member: {kind}")


def _read_plugin_migrations_reply(s: _StructReader, caps: _CapTable) -> Any:
    disc = s.get_u16(0)
    if disc == 0:
        return {"kind": "ok", "value": _plugin_migrations_ok_codec.read(s.get_struct(0, 0, 1), caps)}
    elif disc == 1:
        return {"kind": "err", "value": _plugin_error_codec.read(s.get_struct(0, 0, 2), caps)}
    raise ValueError(f"unknown PluginMigrationsReply union member: {disc}")


_plugin_migrations_reply_codec = _Codec(1, 1, _write_plugin_migrations_reply, _read_plugin_migrations_reply)
"""Wire codec for ``PluginMigrationsReply`` (1 data words, 1 pointers)."""


def _write_byte_source_pull_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_u32(0, v["maxBytes"])


def _read_byte_source_pull_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "maxBytes": s.get_u32(0),
    }


_byte_source_pull_params_codec = _Codec(1, 0, _write_byte_source_pull_params, _read_byte_source_pull_params)
"""Wire codec for ``ByteSourcePullParams`` (1 data words, 0 pointers)."""


def _write_byte_source_pull_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _pull_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_byte_source_pull_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _pull_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_byte_source_pull_results_codec = _Codec(0, 1, _write_byte_source_pull_results, _read_byte_source_pull_results)
"""Wire codec for ``ByteSourcePullResults`` (0 data words, 1 pointers)."""


def _write_destination_head_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["key"])


def _read_destination_head_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "key": s.get_text(0),
    }


_destination_head_params_codec = _Codec(0, 1, _write_destination_head_params, _read_destination_head_params)
"""Wire codec for ``DestinationHeadParams`` (0 data words, 1 pointers)."""


def _write_destination_head_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _head_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_destination_head_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _head_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_destination_head_results_codec = _Codec(0, 1, _write_destination_head_results, _read_destination_head_results)
"""Wire codec for ``DestinationHeadResults`` (0 data words, 1 pointers)."""


def _write_destination_list_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _list_options_codec.write(s.init_struct(0, 1, 2), v["options"], caps)


def _read_destination_list_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "options": _list_options_codec.read(s.get_struct(0, 1, 2), caps),
    }


_destination_list_params_codec = _Codec(0, 1, _write_destination_list_params, _read_destination_list_params)
"""Wire codec for ``DestinationListParams`` (0 data words, 1 pointers)."""


def _write_destination_list_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _list_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_destination_list_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _list_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_destination_list_results_codec = _Codec(0, 1, _write_destination_list_results, _read_destination_list_results)
"""Wire codec for ``DestinationListResults`` (0 data words, 1 pointers)."""


def _write_destination_get_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["key"])
    _read_options_codec.write(s.init_struct(1, 0, 1), v["options"], caps)


def _read_destination_get_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "key": s.get_text(0),
        "options": _read_options_codec.read(s.get_struct(1, 0, 1), caps),
    }


_destination_get_params_codec = _Codec(0, 2, _write_destination_get_params, _read_destination_get_params)
"""Wire codec for ``DestinationGetParams`` (0 data words, 2 pointers)."""


def _write_destination_get_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _get_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_destination_get_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _get_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_destination_get_results_codec = _Codec(0, 1, _write_destination_get_results, _read_destination_get_results)
"""Wire codec for ``DestinationGetResults`` (0 data words, 1 pointers)."""


def _write_destination_put_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["key"])
    s.set_cap(1, caps.export_cap(v["body"]))
    _write_options_codec.write(s.init_struct(2, 2, 3), v["options"], caps)


def _read_destination_put_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "key": s.get_text(0),
        "body": caps.import_cap(s.get_cap_index(1)),
        "options": _write_options_codec.read(s.get_struct(2, 2, 3), caps),
    }


_destination_put_params_codec = _Codec(0, 3, _write_destination_put_params, _read_destination_put_params)
"""Wire codec for ``DestinationPutParams`` (0 data words, 3 pointers)."""


def _write_destination_put_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _put_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_destination_put_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _put_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_destination_put_results_codec = _Codec(0, 1, _write_destination_put_results, _read_destination_put_results)
"""Wire codec for ``DestinationPutResults`` (0 data words, 1 pointers)."""


def _write_destination_copy_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["from"])
    s.set_text(1, v["to"])


def _read_destination_copy_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "from": s.get_text(0),
        "to": s.get_text(1),
    }


_destination_copy_params_codec = _Codec(0, 2, _write_destination_copy_params, _read_destination_copy_params)
"""Wire codec for ``DestinationCopyParams`` (0 data words, 2 pointers)."""


def _write_destination_copy_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _copy_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_destination_copy_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _copy_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_destination_copy_results_codec = _Codec(0, 1, _write_destination_copy_results, _read_destination_copy_results)
"""Wire codec for ``DestinationCopyResults`` (0 data words, 1 pointers)."""


def _write_destination_delete_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["key"])


def _read_destination_delete_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "key": s.get_text(0),
    }


_destination_delete_params_codec = _Codec(0, 1, _write_destination_delete_params, _read_destination_delete_params)
"""Wire codec for ``DestinationDeleteParams`` (0 data words, 1 pointers)."""


def _write_destination_delete_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _empty_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_destination_delete_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _empty_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_destination_delete_results_codec = _Codec(0, 1, _write_destination_delete_results, _read_destination_delete_results)
"""Wire codec for ``DestinationDeleteResults`` (0 data words, 1 pointers)."""


def _write_destination_commit_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["key"])
    s.set_text(1, v["commitToken"])


def _read_destination_commit_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "key": s.get_text(0),
        "commitToken": s.get_text(1),
    }


_destination_commit_params_codec = _Codec(0, 2, _write_destination_commit_params, _read_destination_commit_params)
"""Wire codec for ``DestinationCommitParams`` (0 data words, 2 pointers)."""


def _write_destination_commit_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _put_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_destination_commit_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _put_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_destination_commit_results_codec = _Codec(0, 1, _write_destination_commit_results, _read_destination_commit_results)
"""Wire codec for ``DestinationCommitResults`` (0 data words, 1 pointers)."""


def _write_destination_abort_stage_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["key"])
    s.set_text(1, v["commitToken"])


def _read_destination_abort_stage_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "key": s.get_text(0),
        "commitToken": s.get_text(1),
    }


_destination_abort_stage_params_codec = _Codec(0, 2, _write_destination_abort_stage_params, _read_destination_abort_stage_params)
"""Wire codec for ``DestinationAbortStageParams`` (0 data words, 2 pointers)."""


def _write_destination_abort_stage_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _empty_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_destination_abort_stage_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _empty_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_destination_abort_stage_results_codec = _Codec(0, 1, _write_destination_abort_stage_results, _read_destination_abort_stage_results)
"""Wire codec for ``DestinationAbortStageResults`` (0 data words, 1 pointers)."""


def _write_source_open_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["key"])


def _read_source_open_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "key": s.get_text(0),
    }


_source_open_params_codec = _Codec(0, 1, _write_source_open_params, _read_source_open_params)
"""Wire codec for ``SourceOpenParams`` (0 data words, 1 pointers)."""


def _write_source_open_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _open_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_source_open_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _open_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_source_open_results_codec = _Codec(0, 1, _write_source_open_results, _read_source_open_results)
"""Wire codec for ``SourceOpenResults`` (0 data words, 1 pointers)."""


def _write_progress_sink_report_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_f32(0, v["percent"])
    s.set_text(0, v["message"])


def _read_progress_sink_report_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "percent": s.get_f32(0),
        "message": s.get_text(0),
    }


_progress_sink_report_params_codec = _Codec(1, 1, _write_progress_sink_report_params, _read_progress_sink_report_params)
"""Wire codec for ``ProgressSinkReportParams`` (1 data words, 1 pointers)."""


def _write_progress_sink_report_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _empty_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_progress_sink_report_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _empty_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_progress_sink_report_results_codec = _Codec(0, 1, _write_progress_sink_report_results, _read_progress_sink_report_results)
"""Wire codec for ``ProgressSinkReportResults`` (0 data words, 1 pointers)."""


def _write_cancellation_poll_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_cancellation_poll_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_cancellation_poll_params_codec = _Codec(0, 0, _write_cancellation_poll_params, _read_cancellation_poll_params)
"""Wire codec for ``CancellationPollParams`` (0 data words, 0 pointers)."""


def _write_cancellation_poll_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_bool(0, v["cancelled"])


def _read_cancellation_poll_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "cancelled": s.get_bool(0),
    }


_cancellation_poll_results_codec = _Codec(1, 0, _write_cancellation_poll_results, _read_cancellation_poll_results)
"""Wire codec for ``CancellationPollResults`` (1 data words, 0 pointers)."""


def _write_job_handler_handle_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _job_invocation_codec.write(s.init_struct(0, 3, 8), v["invocation"], caps)
    s.set_cap(1, caps.export_cap(v["input"]))
    s.set_cap(2, caps.export_cap(v["output"]))
    s.set_cap(3, caps.export_cap(v["progress"]))
    s.set_cap(4, caps.export_cap(v["cancel"]))
    s.set_cap(5, caps.export_cap(v["database"]))
    items = s.init_struct_list(6, len(v["databases"]), 0, 2)
    for item, elem in zip(items, v["databases"], strict=True):
        _named_database_codec.write(item, elem, caps)


def _read_job_handler_handle_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "invocation": _job_invocation_codec.read(s.get_struct(0, 3, 8), caps),
        "input": caps.import_cap(s.get_cap_index(1)),
        "output": caps.import_cap(s.get_cap_index(2)),
        "progress": caps.import_cap(s.get_cap_index(3)),
        "cancel": caps.import_cap(s.get_cap_index(4)),
        "database": caps.import_cap(s.get_cap_index(5)),
        "databases": [_named_database_codec.read(item, caps) for item in s.get_struct_list(6, 0, 2)],
    }


_job_handler_handle_params_codec = _Codec(0, 7, _write_job_handler_handle_params, _read_job_handler_handle_params)
"""Wire codec for ``JobHandlerHandleParams`` (0 data words, 7 pointers)."""


def _write_job_handler_handle_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _handle_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_job_handler_handle_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _handle_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_job_handler_handle_results_codec = _Codec(0, 1, _write_job_handler_handle_results, _read_job_handler_handle_results)
"""Wire codec for ``JobHandlerHandleResults`` (0 data words, 1 pointers)."""


def _write_content_source_login_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["paramsJson"])


def _read_content_source_login_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "paramsJson": s.get_text(0),
    }


_content_source_login_params_codec = _Codec(0, 1, _write_content_source_login_params, _read_content_source_login_params)
"""Wire codec for ``ContentSourceLoginParams`` (0 data words, 1 pointers)."""


def _write_content_source_login_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _json_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_content_source_login_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _json_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_content_source_login_results_codec = _Codec(0, 1, _write_content_source_login_results, _read_content_source_login_results)
"""Wire codec for ``ContentSourceLoginResults`` (0 data words, 1 pointers)."""


def _write_content_source_scan_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["paramsJson"])


def _read_content_source_scan_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "paramsJson": s.get_text(0),
    }


_content_source_scan_params_codec = _Codec(0, 1, _write_content_source_scan_params, _read_content_source_scan_params)
"""Wire codec for ``ContentSourceScanParams`` (0 data words, 1 pointers)."""


def _write_content_source_scan_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _json_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_content_source_scan_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _json_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_content_source_scan_results_codec = _Codec(0, 1, _write_content_source_scan_results, _read_content_source_scan_results)
"""Wire codec for ``ContentSourceScanResults`` (0 data words, 1 pointers)."""


def _write_content_source_fetch_title_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["paramsJson"])


def _read_content_source_fetch_title_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "paramsJson": s.get_text(0),
    }


_content_source_fetch_title_params_codec = _Codec(0, 1, _write_content_source_fetch_title_params, _read_content_source_fetch_title_params)
"""Wire codec for ``ContentSourceFetchTitleParams`` (0 data words, 1 pointers)."""


def _write_content_source_fetch_title_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _json_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_content_source_fetch_title_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _json_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_content_source_fetch_title_results_codec = _Codec(0, 1, _write_content_source_fetch_title_results, _read_content_source_fetch_title_results)
"""Wire codec for ``ContentSourceFetchTitleResults`` (0 data words, 1 pointers)."""


def _write_content_source_list_accounts_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_content_source_list_accounts_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_content_source_list_accounts_params_codec = _Codec(0, 0, _write_content_source_list_accounts_params, _read_content_source_list_accounts_params)
"""Wire codec for ``ContentSourceListAccountsParams`` (0 data words, 0 pointers)."""


def _write_content_source_list_accounts_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _json_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_content_source_list_accounts_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _json_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_content_source_list_accounts_results_codec = _Codec(0, 1, _write_content_source_list_accounts_results, _read_content_source_list_accounts_results)
"""Wire codec for ``ContentSourceListAccountsResults`` (0 data words, 1 pointers)."""


def _write_content_source_login_start_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["paramsJson"])


def _read_content_source_login_start_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "paramsJson": s.get_text(0),
    }


_content_source_login_start_params_codec = _Codec(0, 1, _write_content_source_login_start_params, _read_content_source_login_start_params)
"""Wire codec for ``ContentSourceLoginStartParams`` (0 data words, 1 pointers)."""


def _write_content_source_login_start_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _json_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_content_source_login_start_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _json_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_content_source_login_start_results_codec = _Codec(0, 1, _write_content_source_login_start_results, _read_content_source_login_start_results)
"""Wire codec for ``ContentSourceLoginStartResults`` (0 data words, 1 pointers)."""


def _write_content_source_login_complete_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["paramsJson"])


def _read_content_source_login_complete_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "paramsJson": s.get_text(0),
    }


_content_source_login_complete_params_codec = _Codec(0, 1, _write_content_source_login_complete_params, _read_content_source_login_complete_params)
"""Wire codec for ``ContentSourceLoginCompleteParams`` (0 data words, 1 pointers)."""


def _write_content_source_login_complete_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _json_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_content_source_login_complete_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _json_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_content_source_login_complete_results_codec = _Codec(0, 1, _write_content_source_login_complete_results, _read_content_source_login_complete_results)
"""Wire codec for ``ContentSourceLoginCompleteResults`` (0 data words, 1 pointers)."""


def _write_content_source_search_catalog_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["paramsJson"])


def _read_content_source_search_catalog_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "paramsJson": s.get_text(0),
    }


_content_source_search_catalog_params_codec = _Codec(0, 1, _write_content_source_search_catalog_params, _read_content_source_search_catalog_params)
"""Wire codec for ``ContentSourceSearchCatalogParams`` (0 data words, 1 pointers)."""


def _write_content_source_search_catalog_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _json_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_content_source_search_catalog_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _json_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_content_source_search_catalog_results_codec = _Codec(0, 1, _write_content_source_search_catalog_results, _read_content_source_search_catalog_results)
"""Wire codec for ``ContentSourceSearchCatalogResults`` (0 data words, 1 pointers)."""


def _write_content_source_expand_candidates_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["paramsJson"])


def _read_content_source_expand_candidates_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "paramsJson": s.get_text(0),
    }


_content_source_expand_candidates_params_codec = _Codec(0, 1, _write_content_source_expand_candidates_params, _read_content_source_expand_candidates_params)
"""Wire codec for ``ContentSourceExpandCandidatesParams`` (0 data words, 1 pointers)."""


def _write_content_source_expand_candidates_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _json_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_content_source_expand_candidates_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _json_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_content_source_expand_candidates_results_codec = _Codec(0, 1, _write_content_source_expand_candidates_results, _read_content_source_expand_candidates_results)
"""Wire codec for ``ContentSourceExpandCandidatesResults`` (0 data words, 1 pointers)."""


def _write_content_source_purchase_hint_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["paramsJson"])


def _read_content_source_purchase_hint_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "paramsJson": s.get_text(0),
    }


_content_source_purchase_hint_params_codec = _Codec(0, 1, _write_content_source_purchase_hint_params, _read_content_source_purchase_hint_params)
"""Wire codec for ``ContentSourcePurchaseHintParams`` (0 data words, 1 pointers)."""


def _write_content_source_purchase_hint_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _json_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_content_source_purchase_hint_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _json_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_content_source_purchase_hint_results_codec = _Codec(0, 1, _write_content_source_purchase_hint_results, _read_content_source_purchase_hint_results)
"""Wire codec for ``ContentSourcePurchaseHintResults`` (0 data words, 1 pointers)."""


def _write_content_source_list_deals_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["paramsJson"])


def _read_content_source_list_deals_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "paramsJson": s.get_text(0),
    }


_content_source_list_deals_params_codec = _Codec(0, 1, _write_content_source_list_deals_params, _read_content_source_list_deals_params)
"""Wire codec for ``ContentSourceListDealsParams`` (0 data words, 1 pointers)."""


def _write_content_source_list_deals_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _json_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_content_source_list_deals_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _json_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_content_source_list_deals_results_codec = _Codec(0, 1, _write_content_source_list_deals_results, _read_content_source_list_deals_results)
"""Wire codec for ``ContentSourceListDealsResults`` (0 data words, 1 pointers)."""


def _write_content_source_health_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_content_source_health_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_content_source_health_params_codec = _Codec(0, 0, _write_content_source_health_params, _read_content_source_health_params)
"""Wire codec for ``ContentSourceHealthParams`` (0 data words, 0 pointers)."""


def _write_content_source_health_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _health_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_content_source_health_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _health_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_content_source_health_results_codec = _Codec(0, 1, _write_content_source_health_results, _read_content_source_health_results)
"""Wire codec for ``ContentSourceHealthResults`` (0 data words, 1 pointers)."""


def _write_content_source_diagnose_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_content_source_diagnose_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_content_source_diagnose_params_codec = _Codec(0, 0, _write_content_source_diagnose_params, _read_content_source_diagnose_params)
"""Wire codec for ``ContentSourceDiagnoseParams`` (0 data words, 0 pointers)."""


def _write_content_source_diagnose_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _json_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_content_source_diagnose_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _json_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_content_source_diagnose_results_codec = _Codec(0, 1, _write_content_source_diagnose_results, _read_content_source_diagnose_results)
"""Wire codec for ``ContentSourceDiagnoseResults`` (0 data words, 1 pointers)."""


def _write_content_source_catalog_detail_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["paramsJson"])


def _read_content_source_catalog_detail_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "paramsJson": s.get_text(0),
    }


_content_source_catalog_detail_params_codec = _Codec(0, 1, _write_content_source_catalog_detail_params, _read_content_source_catalog_detail_params)
"""Wire codec for ``ContentSourceCatalogDetailParams`` (0 data words, 1 pointers)."""


def _write_content_source_catalog_detail_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _json_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_content_source_catalog_detail_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _json_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_content_source_catalog_detail_results_codec = _Codec(0, 1, _write_content_source_catalog_detail_results, _read_content_source_catalog_detail_results)
"""Wire codec for ``ContentSourceCatalogDetailResults`` (0 data words, 1 pointers)."""


def _write_integration_health_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_integration_health_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_integration_health_params_codec = _Codec(0, 0, _write_integration_health_params, _read_integration_health_params)
"""Wire codec for ``IntegrationHealthParams`` (0 data words, 0 pointers)."""


def _write_integration_health_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _health_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_integration_health_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _health_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_integration_health_results_codec = _Codec(0, 1, _write_integration_health_results, _read_integration_health_results)
"""Wire codec for ``IntegrationHealthResults`` (0 data words, 1 pointers)."""


def _write_integration_on_event_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _domain_event_codec.write(s.init_struct(0, 4, 9), v["event"], caps)


def _read_integration_on_event_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "event": _domain_event_codec.read(s.get_struct(0, 4, 9), caps),
    }


_integration_on_event_params_codec = _Codec(0, 1, _write_integration_on_event_params, _read_integration_on_event_params)
"""Wire codec for ``IntegrationOnEventParams`` (0 data words, 1 pointers)."""


def _write_integration_on_event_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _event_result_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_integration_on_event_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _event_result_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_integration_on_event_results_codec = _Codec(0, 1, _write_integration_on_event_results, _read_integration_on_event_results)
"""Wire codec for ``IntegrationOnEventResults`` (0 data words, 1 pointers)."""


def _write_integration_start_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_integration_start_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_integration_start_params_codec = _Codec(0, 0, _write_integration_start_params, _read_integration_start_params)
"""Wire codec for ``IntegrationStartParams`` (0 data words, 0 pointers)."""


def _write_integration_start_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _empty_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_integration_start_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _empty_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_integration_start_results_codec = _Codec(0, 1, _write_integration_start_results, _read_integration_start_results)
"""Wire codec for ``IntegrationStartResults`` (0 data words, 1 pointers)."""


def _write_integration_stop_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_integration_stop_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_integration_stop_params_codec = _Codec(0, 0, _write_integration_stop_params, _read_integration_stop_params)
"""Wire codec for ``IntegrationStopParams`` (0 data words, 0 pointers)."""


def _write_integration_stop_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _empty_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_integration_stop_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _empty_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_integration_stop_results_codec = _Codec(0, 1, _write_integration_stop_results, _read_integration_stop_results)
"""Wire codec for ``IntegrationStopResults`` (0 data words, 1 pointers)."""


def _write_integration_diagnose_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_integration_diagnose_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_integration_diagnose_params_codec = _Codec(0, 0, _write_integration_diagnose_params, _read_integration_diagnose_params)
"""Wire codec for ``IntegrationDiagnoseParams`` (0 data words, 0 pointers)."""


def _write_integration_diagnose_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _json_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_integration_diagnose_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _json_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_integration_diagnose_results_codec = _Codec(0, 1, _write_integration_diagnose_results, _read_integration_diagnose_results)
"""Wire codec for ``IntegrationDiagnoseResults`` (0 data words, 1 pointers)."""


def _write_integration_scan_library_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["paramsJson"])


def _read_integration_scan_library_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "paramsJson": s.get_text(0),
    }


_integration_scan_library_params_codec = _Codec(0, 1, _write_integration_scan_library_params, _read_integration_scan_library_params)
"""Wire codec for ``IntegrationScanLibraryParams`` (0 data words, 1 pointers)."""


def _write_integration_scan_library_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _empty_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_integration_scan_library_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _empty_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_integration_scan_library_results_codec = _Codec(0, 1, _write_integration_scan_library_results, _read_integration_scan_library_results)
"""Wire codec for ``IntegrationScanLibraryResults`` (0 data words, 1 pointers)."""


def _write_integration_sync_listening_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_integration_sync_listening_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_integration_sync_listening_params_codec = _Codec(0, 0, _write_integration_sync_listening_params, _read_integration_sync_listening_params)
"""Wire codec for ``IntegrationSyncListeningParams`` (0 data words, 0 pointers)."""


def _write_integration_sync_listening_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _json_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_integration_sync_listening_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _json_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_integration_sync_listening_results_codec = _Codec(0, 1, _write_integration_sync_listening_results, _read_integration_sync_listening_results)
"""Wire codec for ``IntegrationSyncListeningResults`` (0 data words, 1 pointers)."""


def _write_integration_authenticate_user_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["paramsJson"])


def _read_integration_authenticate_user_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "paramsJson": s.get_text(0),
    }


_integration_authenticate_user_params_codec = _Codec(0, 1, _write_integration_authenticate_user_params, _read_integration_authenticate_user_params)
"""Wire codec for ``IntegrationAuthenticateUserParams`` (0 data words, 1 pointers)."""


def _write_integration_authenticate_user_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _json_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_integration_authenticate_user_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _json_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_integration_authenticate_user_results_codec = _Codec(0, 1, _write_integration_authenticate_user_results, _read_integration_authenticate_user_results)
"""Wire codec for ``IntegrationAuthenticateUserResults`` (0 data words, 1 pointers)."""


def _write_integration_poll_events_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_integration_poll_events_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_integration_poll_events_params_codec = _Codec(0, 0, _write_integration_poll_events_params, _read_integration_poll_events_params)
"""Wire codec for ``IntegrationPollEventsParams`` (0 data words, 0 pointers)."""


def _write_integration_poll_events_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _json_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_integration_poll_events_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _json_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_integration_poll_events_results_codec = _Codec(0, 1, _write_integration_poll_events_results, _read_integration_poll_events_results)
"""Wire codec for ``IntegrationPollEventsResults`` (0 data words, 1 pointers)."""


def _write_database_open_session_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_database_open_session_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_database_open_session_params_codec = _Codec(0, 0, _write_database_open_session_params, _read_database_open_session_params)
"""Wire codec for ``DatabaseOpenSessionParams`` (0 data words, 0 pointers)."""


def _write_database_open_session_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _adapter_session_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_database_open_session_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _adapter_session_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_database_open_session_results_codec = _Codec(0, 1, _write_database_open_session_results, _read_database_open_session_results)
"""Wire codec for ``DatabaseOpenSessionResults`` (0 data words, 1 pointers)."""


def _write_adapter_database_session_capabilities_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_adapter_database_session_capabilities_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_adapter_database_session_capabilities_params_codec = _Codec(0, 0, _write_adapter_database_session_capabilities_params, _read_adapter_database_session_capabilities_params)
"""Wire codec for ``AdapterDatabaseSessionCapabilitiesParams`` (0 data words, 0 pointers)."""


def _write_adapter_database_session_capabilities_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _db_capabilities_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_adapter_database_session_capabilities_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _db_capabilities_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_adapter_database_session_capabilities_results_codec = _Codec(0, 1, _write_adapter_database_session_capabilities_results, _read_adapter_database_session_capabilities_results)
"""Wire codec for ``AdapterDatabaseSessionCapabilitiesResults`` (0 data words, 1 pointers)."""


def _write_adapter_database_session_execute_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _adapter_execute_request_codec.write(s.init_struct(0, 2, 4), v["request"], caps)


def _read_adapter_database_session_execute_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "request": _adapter_execute_request_codec.read(s.get_struct(0, 2, 4), caps),
    }


_adapter_database_session_execute_params_codec = _Codec(0, 1, _write_adapter_database_session_execute_params, _read_adapter_database_session_execute_params)
"""Wire codec for ``AdapterDatabaseSessionExecuteParams`` (0 data words, 1 pointers)."""


def _write_adapter_database_session_execute_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _execute_result_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_adapter_database_session_execute_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _execute_result_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_adapter_database_session_execute_results_codec = _Codec(0, 1, _write_adapter_database_session_execute_results, _read_adapter_database_session_execute_results)
"""Wire codec for ``AdapterDatabaseSessionExecuteResults`` (0 data words, 1 pointers)."""


def _write_adapter_database_session_close_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_adapter_database_session_close_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_adapter_database_session_close_params_codec = _Codec(0, 0, _write_adapter_database_session_close_params, _read_adapter_database_session_close_params)
"""Wire codec for ``AdapterDatabaseSessionCloseParams`` (0 data words, 0 pointers)."""


def _write_adapter_database_session_close_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _empty_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_adapter_database_session_close_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _empty_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_adapter_database_session_close_results_codec = _Codec(0, 1, _write_adapter_database_session_close_results, _read_adapter_database_session_close_results)
"""Wire codec for ``AdapterDatabaseSessionCloseResults`` (0 data words, 1 pointers)."""


def _write_adapter_database_session_bootstrap_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_adapter_database_session_bootstrap_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_adapter_database_session_bootstrap_params_codec = _Codec(0, 0, _write_adapter_database_session_bootstrap_params, _read_adapter_database_session_bootstrap_params)
"""Wire codec for ``AdapterDatabaseSessionBootstrapParams`` (0 data words, 0 pointers)."""


def _write_adapter_database_session_bootstrap_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _db_bootstrap_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_adapter_database_session_bootstrap_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _db_bootstrap_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_adapter_database_session_bootstrap_results_codec = _Codec(0, 1, _write_adapter_database_session_bootstrap_results, _read_adapter_database_session_bootstrap_results)
"""Wire codec for ``AdapterDatabaseSessionBootstrapResults`` (0 data words, 1 pointers)."""


def _write_adapter_database_session_export_identity_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_adapter_database_session_export_identity_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_adapter_database_session_export_identity_params_codec = _Codec(0, 0, _write_adapter_database_session_export_identity_params, _read_adapter_database_session_export_identity_params)
"""Wire codec for ``AdapterDatabaseSessionExportIdentityParams`` (0 data words, 0 pointers)."""


def _write_adapter_database_session_export_identity_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _identity_export_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_adapter_database_session_export_identity_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _identity_export_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_adapter_database_session_export_identity_results_codec = _Codec(0, 1, _write_adapter_database_session_export_identity_results, _read_adapter_database_session_export_identity_results)
"""Wire codec for ``AdapterDatabaseSessionExportIdentityResults`` (0 data words, 1 pointers)."""


def _write_adapter_database_session_import_identity_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    items = s.init_struct_list(0, len(v["rows"]), 1, 1)
    for item, elem in zip(items, v["rows"], strict=True):
        _identity_high_water_codec.write(item, elem, caps)


def _read_adapter_database_session_import_identity_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "rows": [_identity_high_water_codec.read(item, caps) for item in s.get_struct_list(0, 1, 1)],
    }


_adapter_database_session_import_identity_params_codec = _Codec(0, 1, _write_adapter_database_session_import_identity_params, _read_adapter_database_session_import_identity_params)
"""Wire codec for ``AdapterDatabaseSessionImportIdentityParams`` (0 data words, 1 pointers)."""


def _write_adapter_database_session_import_identity_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _empty_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_adapter_database_session_import_identity_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _empty_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_adapter_database_session_import_identity_results_codec = _Codec(0, 1, _write_adapter_database_session_import_identity_results, _read_adapter_database_session_import_identity_results)
"""Wire codec for ``AdapterDatabaseSessionImportIdentityResults`` (0 data words, 1 pointers)."""


def _write_adapter_database_session_list_user_relations_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_adapter_database_session_list_user_relations_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_adapter_database_session_list_user_relations_params_codec = _Codec(0, 0, _write_adapter_database_session_list_user_relations_params, _read_adapter_database_session_list_user_relations_params)
"""Wire codec for ``AdapterDatabaseSessionListUserRelationsParams`` (0 data words, 0 pointers)."""


def _write_adapter_database_session_list_user_relations_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _user_relations_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_adapter_database_session_list_user_relations_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _user_relations_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_adapter_database_session_list_user_relations_results_codec = _Codec(0, 1, _write_adapter_database_session_list_user_relations_results, _read_adapter_database_session_list_user_relations_results)
"""Wire codec for ``AdapterDatabaseSessionListUserRelationsResults`` (0 data words, 1 pointers)."""


def _write_adapter_database_session_prepare_unit_restore_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_adapter_database_session_prepare_unit_restore_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_adapter_database_session_prepare_unit_restore_params_codec = _Codec(0, 0, _write_adapter_database_session_prepare_unit_restore_params, _read_adapter_database_session_prepare_unit_restore_params)
"""Wire codec for ``AdapterDatabaseSessionPrepareUnitRestoreParams`` (0 data words, 0 pointers)."""


def _write_adapter_database_session_prepare_unit_restore_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _empty_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_adapter_database_session_prepare_unit_restore_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _empty_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_adapter_database_session_prepare_unit_restore_results_codec = _Codec(0, 1, _write_adapter_database_session_prepare_unit_restore_results, _read_adapter_database_session_prepare_unit_restore_results)
"""Wire codec for ``AdapterDatabaseSessionPrepareUnitRestoreResults`` (0 data words, 1 pointers)."""


def _write_adapter_database_session_drop_user_relations_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text_list(0, v["names"])


def _read_adapter_database_session_drop_user_relations_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "names": s.get_text_list(0),
    }


_adapter_database_session_drop_user_relations_params_codec = _Codec(0, 1, _write_adapter_database_session_drop_user_relations_params, _read_adapter_database_session_drop_user_relations_params)
"""Wire codec for ``AdapterDatabaseSessionDropUserRelationsParams`` (0 data words, 1 pointers)."""


def _write_adapter_database_session_drop_user_relations_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _empty_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_adapter_database_session_drop_user_relations_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _empty_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_adapter_database_session_drop_user_relations_results_codec = _Codec(0, 1, _write_adapter_database_session_drop_user_relations_results, _read_adapter_database_session_drop_user_relations_results)
"""Wire codec for ``AdapterDatabaseSessionDropUserRelationsResults`` (0 data words, 1 pointers)."""


def _write_adapter_database_session_assert_restore_constraints_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_adapter_database_session_assert_restore_constraints_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_adapter_database_session_assert_restore_constraints_params_codec = _Codec(0, 0, _write_adapter_database_session_assert_restore_constraints_params, _read_adapter_database_session_assert_restore_constraints_params)
"""Wire codec for ``AdapterDatabaseSessionAssertRestoreConstraintsParams`` (0 data words, 0 pointers)."""


def _write_adapter_database_session_assert_restore_constraints_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _empty_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_adapter_database_session_assert_restore_constraints_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _empty_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_adapter_database_session_assert_restore_constraints_results_codec = _Codec(0, 1, _write_adapter_database_session_assert_restore_constraints_results, _read_adapter_database_session_assert_restore_constraints_results)
"""Wire codec for ``AdapterDatabaseSessionAssertRestoreConstraintsResults`` (0 data words, 1 pointers)."""


def _write_guest_database_execute_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _execute_request_codec.write(s.init_struct(0, 1, 3), v["request"], caps)


def _read_guest_database_execute_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "request": _execute_request_codec.read(s.get_struct(0, 1, 3), caps),
    }


_guest_database_execute_params_codec = _Codec(0, 1, _write_guest_database_execute_params, _read_guest_database_execute_params)
"""Wire codec for ``GuestDatabaseExecuteParams`` (0 data words, 1 pointers)."""


def _write_guest_database_execute_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _execute_result_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_guest_database_execute_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _execute_result_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_guest_database_execute_results_codec = _Codec(0, 1, _write_guest_database_execute_results, _read_guest_database_execute_results)
"""Wire codec for ``GuestDatabaseExecuteResults`` (0 data words, 1 pointers)."""


def _write_guest_database_close_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_guest_database_close_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_guest_database_close_params_codec = _Codec(0, 0, _write_guest_database_close_params, _read_guest_database_close_params)
"""Wire codec for ``GuestDatabaseCloseParams`` (0 data words, 0 pointers)."""


def _write_guest_database_close_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _empty_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_guest_database_close_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _empty_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_guest_database_close_results_codec = _Codec(0, 1, _write_guest_database_close_results, _read_guest_database_close_results)
"""Wire codec for ``GuestDatabaseCloseResults`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_describe_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_bookclerk_plugin_describe_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_bookclerk_plugin_describe_params_codec = _Codec(0, 0, _write_bookclerk_plugin_describe_params, _read_bookclerk_plugin_describe_params)
"""Wire codec for ``BookclerkPluginDescribeParams`` (0 data words, 0 pointers)."""


def _write_bookclerk_plugin_describe_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _describe_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_bookclerk_plugin_describe_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _describe_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_bookclerk_plugin_describe_results_codec = _Codec(0, 1, _write_bookclerk_plugin_describe_results, _read_bookclerk_plugin_describe_results)
"""Wire codec for ``BookclerkPluginDescribeResults`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_destination_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _destination_context_codec.write(s.init_struct(0, 0, 2), v["context"], caps)


def _read_bookclerk_plugin_destination_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "context": _destination_context_codec.read(s.get_struct(0, 0, 2), caps),
    }


_bookclerk_plugin_destination_params_codec = _Codec(0, 1, _write_bookclerk_plugin_destination_params, _read_bookclerk_plugin_destination_params)
"""Wire codec for ``BookclerkPluginDestinationParams`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_destination_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _destination_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_bookclerk_plugin_destination_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _destination_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_bookclerk_plugin_destination_results_codec = _Codec(0, 1, _write_bookclerk_plugin_destination_results, _read_bookclerk_plugin_destination_results)
"""Wire codec for ``BookclerkPluginDestinationResults`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_source_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _source_context_codec.write(s.init_struct(0, 0, 2), v["context"], caps)


def _read_bookclerk_plugin_source_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "context": _source_context_codec.read(s.get_struct(0, 0, 2), caps),
    }


_bookclerk_plugin_source_params_codec = _Codec(0, 1, _write_bookclerk_plugin_source_params, _read_bookclerk_plugin_source_params)
"""Wire codec for ``BookclerkPluginSourceParams`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_source_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _source_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_bookclerk_plugin_source_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _source_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_bookclerk_plugin_source_results_codec = _Codec(0, 1, _write_bookclerk_plugin_source_results, _read_bookclerk_plugin_source_results)
"""Wire codec for ``BookclerkPluginSourceResults`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_worker_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _worker_context_codec.write(s.init_struct(0, 0, 3), v["context"], caps)


def _read_bookclerk_plugin_worker_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "context": _worker_context_codec.read(s.get_struct(0, 0, 3), caps),
    }


_bookclerk_plugin_worker_params_codec = _Codec(0, 1, _write_bookclerk_plugin_worker_params, _read_bookclerk_plugin_worker_params)
"""Wire codec for ``BookclerkPluginWorkerParams`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_worker_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _worker_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_bookclerk_plugin_worker_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _worker_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_bookclerk_plugin_worker_results_codec = _Codec(0, 1, _write_bookclerk_plugin_worker_results, _read_bookclerk_plugin_worker_results)
"""Wire codec for ``BookclerkPluginWorkerResults`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_shutdown_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_bookclerk_plugin_shutdown_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_bookclerk_plugin_shutdown_params_codec = _Codec(0, 0, _write_bookclerk_plugin_shutdown_params, _read_bookclerk_plugin_shutdown_params)
"""Wire codec for ``BookclerkPluginShutdownParams`` (0 data words, 0 pointers)."""


def _write_bookclerk_plugin_shutdown_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _empty_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_bookclerk_plugin_shutdown_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _empty_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_bookclerk_plugin_shutdown_results_codec = _Codec(0, 1, _write_bookclerk_plugin_shutdown_results, _read_bookclerk_plugin_shutdown_results)
"""Wire codec for ``BookclerkPluginShutdownResults`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_content_source_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _content_source_context_codec.write(s.init_struct(0, 0, 2), v["context"], caps)


def _read_bookclerk_plugin_content_source_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "context": _content_source_context_codec.read(s.get_struct(0, 0, 2), caps),
    }


_bookclerk_plugin_content_source_params_codec = _Codec(0, 1, _write_bookclerk_plugin_content_source_params, _read_bookclerk_plugin_content_source_params)
"""Wire codec for ``BookclerkPluginContentSourceParams`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_content_source_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _content_source_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_bookclerk_plugin_content_source_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _content_source_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_bookclerk_plugin_content_source_results_codec = _Codec(0, 1, _write_bookclerk_plugin_content_source_results, _read_bookclerk_plugin_content_source_results)
"""Wire codec for ``BookclerkPluginContentSourceResults`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_integration_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _integration_context_codec.write(s.init_struct(0, 0, 2), v["context"], caps)


def _read_bookclerk_plugin_integration_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "context": _integration_context_codec.read(s.get_struct(0, 0, 2), caps),
    }


_bookclerk_plugin_integration_params_codec = _Codec(0, 1, _write_bookclerk_plugin_integration_params, _read_bookclerk_plugin_integration_params)
"""Wire codec for ``BookclerkPluginIntegrationParams`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_integration_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _integration_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_bookclerk_plugin_integration_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _integration_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_bookclerk_plugin_integration_results_codec = _Codec(0, 1, _write_bookclerk_plugin_integration_results, _read_bookclerk_plugin_integration_results)
"""Wire codec for ``BookclerkPluginIntegrationResults`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_database_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _database_context_codec.write(s.init_struct(0, 0, 2), v["context"], caps)


def _read_bookclerk_plugin_database_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "context": _database_context_codec.read(s.get_struct(0, 0, 2), caps),
    }


_bookclerk_plugin_database_params_codec = _Codec(0, 1, _write_bookclerk_plugin_database_params, _read_bookclerk_plugin_database_params)
"""Wire codec for ``BookclerkPluginDatabaseParams`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_database_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _database_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_bookclerk_plugin_database_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _database_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_bookclerk_plugin_database_results_codec = _Codec(0, 1, _write_bookclerk_plugin_database_results, _read_bookclerk_plugin_database_results)
"""Wire codec for ``BookclerkPluginDatabaseResults`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_cli_describe_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_bookclerk_plugin_cli_describe_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_bookclerk_plugin_cli_describe_params_codec = _Codec(0, 0, _write_bookclerk_plugin_cli_describe_params, _read_bookclerk_plugin_cli_describe_params)
"""Wire codec for ``BookclerkPluginCliDescribeParams`` (0 data words, 0 pointers)."""


def _write_bookclerk_plugin_cli_describe_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _json_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_bookclerk_plugin_cli_describe_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _json_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_bookclerk_plugin_cli_describe_results_codec = _Codec(0, 1, _write_bookclerk_plugin_cli_describe_results, _read_bookclerk_plugin_cli_describe_results)
"""Wire codec for ``BookclerkPluginCliDescribeResults`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_cli_invoke_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["paramsJson"])


def _read_bookclerk_plugin_cli_invoke_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "paramsJson": s.get_text(0),
    }


_bookclerk_plugin_cli_invoke_params_codec = _Codec(0, 1, _write_bookclerk_plugin_cli_invoke_params, _read_bookclerk_plugin_cli_invoke_params)
"""Wire codec for ``BookclerkPluginCliInvokeParams`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_cli_invoke_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _json_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_bookclerk_plugin_cli_invoke_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _json_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_bookclerk_plugin_cli_invoke_results_codec = _Codec(0, 1, _write_bookclerk_plugin_cli_invoke_results, _read_bookclerk_plugin_cli_invoke_results)
"""Wire codec for ``BookclerkPluginCliInvokeResults`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_oidc_clients_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    pass


def _read_bookclerk_plugin_oidc_clients_params(s: _StructReader, caps: _CapTable) -> Any:
    return {}


_bookclerk_plugin_oidc_clients_params_codec = _Codec(0, 0, _write_bookclerk_plugin_oidc_clients_params, _read_bookclerk_plugin_oidc_clients_params)
"""Wire codec for ``BookclerkPluginOidcClientsParams`` (0 data words, 0 pointers)."""


def _write_bookclerk_plugin_oidc_clients_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _oidc_clients_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_bookclerk_plugin_oidc_clients_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _oidc_clients_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_bookclerk_plugin_oidc_clients_results_codec = _Codec(0, 1, _write_bookclerk_plugin_oidc_clients_results, _read_bookclerk_plugin_oidc_clients_results)
"""Wire codec for ``BookclerkPluginOidcClientsResults`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_database_migrations_params(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    s.set_text(0, v["binding"])


def _read_bookclerk_plugin_database_migrations_params(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "binding": s.get_text(0),
    }


_bookclerk_plugin_database_migrations_params_codec = _Codec(0, 1, _write_bookclerk_plugin_database_migrations_params, _read_bookclerk_plugin_database_migrations_params)
"""Wire codec for ``BookclerkPluginDatabaseMigrationsParams`` (0 data words, 1 pointers)."""


def _write_bookclerk_plugin_database_migrations_results(s: _CapnpStruct, v: Any, caps: _CapTable) -> None:
    _plugin_migrations_reply_codec.write(s.init_struct(0, 1, 1), v["result"], caps)


def _read_bookclerk_plugin_database_migrations_results(s: _StructReader, caps: _CapTable) -> Any:
    return {
        "result": _plugin_migrations_reply_codec.read(s.get_struct(0, 1, 1), caps),
    }


_bookclerk_plugin_database_migrations_results_codec = _Codec(0, 1, _write_bookclerk_plugin_database_migrations_results, _read_bookclerk_plugin_database_migrations_results)
"""Wire codec for ``BookclerkPluginDatabaseMigrationsResults`` (0 data words, 1 pointers)."""
