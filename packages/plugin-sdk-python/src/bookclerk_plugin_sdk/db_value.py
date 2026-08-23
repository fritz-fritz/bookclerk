"""Universal Cap'n database value domain (``DbValue``) and unpacked codec.

Baseline cells are typed null, bool, int64 (``int``), finite float64, UTF-8
text, and bytes (``bytes``). Unknown ``kind`` values fail closed. JSON
:func:`parse_db_value` still accepts a ``b64:`` string for bytes; the codec
always uses the domain types.
"""

from __future__ import annotations

import hashlib
import math
import struct
import uuid
from typing import Any, Callable, Literal, TypedDict, Union

DbType = Literal["unspecified", "bool", "int64", "float64", "text", "bytes"]

KINDS = frozenset({"null", "boolean", "int64", "float64", "text", "bytes"})
TYPES = frozenset({"unspecified", "bool", "int64", "float64", "text", "bytes"})

I64_MIN = -(2**63)
I64_MAX = 2**63 - 1

_DB_TYPE_ORD = {
    "unspecified": 0,
    "bool": 1,
    "int64": 2,
    "float64": 3,
    "text": 4,
    "bytes": 5,
}
_DB_TYPE_FROM = (
    "unspecified",
    "bool",
    "int64",
    "float64",
    "text",
    "bytes",
)

_KIND_ORD = {"query": 0, "execute": 1, "select": 2, "returning": 3}
_KIND_FROM = ("query", "execute", "select", "returning")
_SELECT_ORD = {"discard": 0, "affectedRows": 1, "rows": 2, "cursor": 3}
_SELECT_FROM = ("discard", "affectedRows", "rows", "cursor")

WORD = 8


class NullValue(TypedDict):
    """Typed SQL NULL with a declared ``DbType``."""

    kind: Literal["null"]
    value: DbType


class BoolValue(TypedDict):
    """Boolean cell."""

    kind: Literal["boolean"]
    value: bool


class Int64Value(TypedDict):
    """Signed 64-bit integer cell (arbitrary-precision ``int`` in range)."""

    kind: Literal["int64"]
    value: int


class Float64Value(TypedDict):
    """Finite IEEE-754 float64 cell."""

    kind: Literal["float64"]
    value: float


class TextValue(TypedDict):
    """UTF-8 text cell (embedded NUL allowed)."""

    kind: Literal["text"]
    value: str


class BytesValue(TypedDict):
    """Binary cell as raw ``bytes`` (not a ``b64:`` string)."""

    kind: Literal["bytes"]
    value: bytes


DbValue = Union[NullValue, BoolValue, Int64Value, Float64Value, TextValue, BytesValue]

DbStatementKind = Literal["query", "execute", "select", "returning"]
DbResultSelection = Literal["discard", "affectedRows", "rows", "cursor"]


class TypedDbStatement(TypedDict):
    """One statement in a typed atomic batch."""

    sql: str
    parameters: list[DbValue]
    kind: DbStatementKind
    maxRows: int
    resultSelection: DbResultSelection


class ExecuteRequest(TypedDict):
    """Typed ``DatabaseSession.executeAtomic`` request."""

    operationId: str
    requestHash: str
    statements: list[TypedDbStatement]
    outcomeIndex: int
    payloadIndex: int
    hasPayloadIndex: bool
    priorReceiptIndex: int
    hasPriorReceiptIndex: bool
    receiptSelectIndex: int
    hasReceiptSelectIndex: bool
    deadlineUnixMs: int


class StatementResult(TypedDict):
    """Result of one statement in an atomic batch."""

    rows: list[dict[str, Any]]
    columns: list[dict[str, Any]]
    rowsAffected: int
    cursor: str


class DbTiming(TypedDict):
    """Handler/engine timing on ``ExecuteReply``."""

    attemptElapsedUs: int
    dbExecutionUs: int
    dbTimingSource: str


class ExecuteReply(TypedDict):
    """Typed ``executeAtomic`` reply."""

    operationId: str
    statements: list[StatementResult]
    timing: DbTiming


def parse_db_value(raw: Any) -> DbValue:
    """Parse a JSON ``DbValue``. Unknown union members raise ``ValueError``.

    ``bytes`` accepts raw ``bytes`` or a ``b64:`` JSON string. ``int64`` is a
    Python ``int`` (not a float).
    """
    if not isinstance(raw, dict) or "kind" not in raw:
        raise ValueError("DbValue must be an object with kind")
    kind = raw["kind"]
    if kind not in KINDS:
        raise ValueError(f"unknown DbValue union member: {kind}")
    value = raw.get("value")
    if kind == "null":
        if value not in TYPES:
            raise ValueError("typed null requires a DbType")
        return {"kind": "null", "value": value}
    if kind == "boolean":
        if not isinstance(value, bool):
            raise ValueError("boolean DbValue requires a bool")
        return {"kind": "boolean", "value": value}
    if kind == "int64":
        if not isinstance(value, int) or isinstance(value, bool):
            raise ValueError("int64 DbValue requires an int")
        if value < I64_MIN or value > I64_MAX:
            raise ValueError("int64 DbValue is out of range")
        return {"kind": "int64", "value": value}
    if kind == "float64":
        if not isinstance(value, (int, float)) or isinstance(value, bool):
            raise ValueError("float64 DbValue requires a number")
        number = float(value)
        if number != number or number in (float("inf"), float("-inf")):
            raise ValueError("float64 value is not finite")
        return {"kind": "float64", "value": number}
    if kind == "text":
        if not isinstance(value, str):
            raise ValueError("text DbValue requires a string")
        return {"kind": "text", "value": value}
    if kind == "bytes":
        return {"kind": "bytes", "value": _parse_bytes(value)}
    raise ValueError(f"unknown DbValue union member: {kind}")


def encode_db_value(value: DbValue) -> bytes:
    """Encode a standalone unpacked Cap'n ``DbValue`` message.

    Args:
        value: Domain value (``int`` / ``bytes``, not a JSON ``b64:`` string).

    Returns:
        Unpacked Cap'n stream bytes (same encoding as the Rust SDK).
    """
    msg = _CapnpMessage()
    root = msg.init_root(2, 1)
    _write_db_value(root, value)
    return msg.finish()


def decode_db_value(data: bytes) -> DbValue:
    """Decode a standalone unpacked Cap'n ``DbValue`` message.

    Args:
        data: Unpacked Cap'n stream.

    Returns:
        Domain value.

    Raises:
        ValueError: If the buffer is not a valid ``DbValue``.
    """
    reader = _CapnpReader(data)
    return _read_db_value(reader.root(2, 1))


def encode_execute_request(request: ExecuteRequest) -> bytes:
    """Encode a standalone unpacked Cap'n ``ExecuteRequest`` message.

    Args:
        request: Structured request (non-empty ``statements``).

    Returns:
        Unpacked Cap'n stream bytes (same encoding as the Rust SDK).

    Raises:
        ValueError: If ``statements`` is empty.
    """
    if not request["statements"]:
        raise ValueError("executeAtomic statements must be non-empty")
    msg = _CapnpMessage()
    root = msg.init_root(4, 3)
    root.set_text(0, request["operationId"])
    root.set_text(1, request["requestHash"])
    root.set_u32(0, request["outcomeIndex"])
    root.set_u32(1, request["payloadIndex"])
    root.set_bool(64, request["hasPayloadIndex"])
    root.set_u32(3, request["priorReceiptIndex"])
    root.set_bool(65, request["hasPriorReceiptIndex"])
    root.set_u32(4, request["receiptSelectIndex"])
    root.set_bool(66, request["hasReceiptSelectIndex"])
    root.set_u64(3, request["deadlineUnixMs"])
    stmts = root.init_struct_list(2, len(request["statements"]), 1, 2)
    for i, stmt in enumerate(request["statements"]):
        _write_statement(stmts[i], stmt)
    return msg.finish()


def decode_execute_request(data: bytes) -> ExecuteRequest:
    """Decode a standalone unpacked Cap'n ``ExecuteRequest`` message.

    Args:
        data: Unpacked Cap'n stream.

    Returns:
        Structured request.

    Raises:
        ValueError: If the buffer is not a valid non-empty ``ExecuteRequest``.
    """
    reader = _CapnpReader(data)
    root = reader.root(4, 3)
    stmt_structs = root.get_struct_list(2, 1, 2)
    if not stmt_structs:
        raise ValueError("executeAtomic statements must be non-empty")
    return {
        "operationId": root.get_text(0),
        "requestHash": root.get_text(1),
        "statements": [_read_statement(s) for s in stmt_structs],
        "outcomeIndex": root.get_u32(0),
        "payloadIndex": root.get_u32(1),
        "hasPayloadIndex": root.get_bool(64),
        "priorReceiptIndex": root.get_u32(3),
        "hasPriorReceiptIndex": root.get_bool(65),
        "receiptSelectIndex": root.get_u32(4),
        "hasReceiptSelectIndex": root.get_bool(66),
        "deadlineUnixMs": root.get_u64(3),
    }


class DatabaseBinding:
    """Host-mediated Cloudflare-style SQL binding for plugin guests.

    Public surface is ``prepare().bind().run()/first()/all()`` and
    ``batch()``. Raw ``executeAtomic`` is an internal transport used by those
    methods. Each call without an explicit :class:`RetryToken` mints a fresh
    operation id and leaves ``requestHash`` empty so the trusted host can stamp
    the canonical digest after validation. A :class:`RetryToken` reuses both.
    """

    def __init__(
        self,
        execute_atomic: Callable[[ExecuteRequest], ExecuteReply],
        *,
        max_request_bytes: int = 0,
        operation_id: str | None = None,
        request_hash: str = "",
        deadline_unix_ms: int = 0,
    ) -> None:
        """Create a binding over a host ``executeAtomic`` transport.

        Args:
            execute_atomic: Host session projection.
            max_request_bytes: Negotiated cap (``0`` = unlimited).
            operation_id: Default retry id; omitted calls mint a UUID.
            request_hash: Default retry hash; empty lets the host stamp one.
            deadline_unix_ms: Guest-visible deadline (unix ms).
        """
        self._execute_atomic = execute_atomic
        self._max_request_bytes = max_request_bytes
        self._operation_id = operation_id
        self._request_hash = request_hash
        self._deadline_unix_ms = deadline_unix_ms

    def prepare(self, sql: str) -> PreparedStatement:
        """Prepare one canonical-SQL statement (``?`` placeholders).

        Args:
            sql: Host-mediated SQL. Kind and bounds are derived by the host.

        Returns:
            A statement that can be bound and executed.
        """
        return PreparedStatement(self, sql, [])

    def batch(
        self,
        statements: list[PreparedStatement],
        *,
        retry: RetryToken | None = None,
        result_selection: DbResultSelection = "rows",
    ) -> ExecuteReply:
        """Run ``statements`` as one typed atomic batch.

        Args:
            statements: Prepared statements (binds already applied).
            retry: Reuse a prior operation id and request hash.
            result_selection: Default selection when a statement has none.

        Returns:
            Decoded ``ExecuteReply``.
        """
        typed = [
            stmt._as_typed(stmt._result_selection or result_selection)  # noqa: SLF001
            for stmt in statements
        ]
        return self._execute_atomic_batch(typed, retry=retry)

    def execute(
        self,
        batch: list[TypedDbStatement],
        *,
        retry: RetryToken | None = None,
    ) -> ExecuteReply:
        """Internal typed-batch transport. Prefer :meth:`prepare` / :meth:`batch`."""
        return self._execute_atomic_batch(batch, retry=retry)

    def _execute_atomic_batch(
        self,
        batch: list[TypedDbStatement],
        *,
        retry: RetryToken | None = None,
    ) -> ExecuteReply:
        if not batch:
            raise ValueError("executeAtomic statements must be non-empty")
        if retry is not None:
            operation_id = retry.operation_id
            request_hash = retry.request_hash
        elif self._operation_id is not None:
            operation_id = self._operation_id
            request_hash = self._request_hash
        else:
            operation_id = str(uuid.uuid4())
            request_hash = ""
        request: ExecuteRequest = {
            "operationId": operation_id,
            "requestHash": request_hash,
            "statements": batch,
            "outcomeIndex": 0,
            "payloadIndex": 0,
            "hasPayloadIndex": False,
            "priorReceiptIndex": 0,
            "hasPriorReceiptIndex": False,
            "receiptSelectIndex": 0,
            "hasReceiptSelectIndex": False,
            "deadlineUnixMs": self._deadline_unix_ms,
        }
        encoded = encode_execute_request(request)
        if self._max_request_bytes and len(encoded) > self._max_request_bytes:
            raise ValueError(
                f"atomic request is {len(encoded)} bytes; guest maxRequestBytes is "
                f"{self._max_request_bytes}"
            )
        return self._execute_atomic(request)


class RetryToken:
    """Explicit retry identity: reuse both ``operationId`` and ``requestHash``."""

    def __init__(self, operation_id: str, request_hash: str) -> None:
        self.operation_id = operation_id
        self.request_hash = request_hash


class PreparedStatement:
    """Cloudflare-style prepared statement over a :class:`DatabaseBinding`."""

    def __init__(
        self,
        binding: DatabaseBinding,
        sql: str,
        parameters: list[DbValue],
        result_selection: DbResultSelection | None = None,
    ) -> None:
        self._binding = binding
        self.sql = sql
        self.parameters = list(parameters)
        self._result_selection = result_selection

    def bind(self, *values: DbValue) -> PreparedStatement:
        """Replace bound parameters with ``values`` (universal ``DbValue`` only)."""
        return PreparedStatement(self._binding, self.sql, list(values), self._result_selection)

    def run(self, *, retry: RetryToken | None = None) -> ExecuteReply:
        """Execute as DML (``affectedRows``)."""
        return self._binding.batch([self], retry=retry, result_selection="affectedRows")

    def first(self, *, retry: RetryToken | None = None) -> dict[str, Any] | None:
        """Return the first row as a name→value map, or ``None``."""
        reply = self._binding.batch([self], retry=retry, result_selection="rows")
        result = reply["statements"][0] if reply["statements"] else None
        if result is None or not result["rows"]:
            return None
        columns = result["columns"]
        row = result["rows"][0]
        cells = row["values"] if isinstance(row, dict) else []
        return {col["name"]: cell for col, cell in zip(columns, cells)}

    def all(self, *, retry: RetryToken | None = None) -> ExecuteReply:
        """Execute as a row-returning query."""
        return self._binding.batch([self], retry=retry, result_selection="rows")

    def _as_typed(self, result_selection: DbResultSelection) -> TypedDbStatement:
        kind: DbStatementKind = (
            "execute" if result_selection in ("affectedRows", "discard") else "select"
        )
        return {
            "sql": self.sql,
            "parameters": self.parameters,
            "kind": kind,
            "maxRows": 0,
            "resultSelection": result_selection,
        }


def canonical_execute_request_hash(request: ExecuteRequest) -> str:
    """SHA-256 hex of the Cap'n request with transport metadata cleared.

    ``operationId``, ``requestHash``, and ``deadlineUnixMs`` are omitted so a
    retry can refresh the remaining deadline. Matches the host digest.
    """
    canonical: ExecuteRequest = {
        **request,
        "operationId": "",
        "requestHash": "",
        "deadlineUnixMs": 0,
    }
    return hashlib.sha256(encode_execute_request(canonical)).hexdigest()


def _parse_bytes(value: Any) -> bytes:
    if isinstance(value, (bytes, bytearray)):
        return bytes(value)
    if not isinstance(value, str) or not value.startswith("b64:"):
        raise ValueError("bytes DbValue requires bytes")
    import base64

    return base64.b64decode(value[4:])


class _CapnpMessage:
    """Single-segment unpacked Cap'n builder."""
    def __init__(self) -> None:
        self._buf = bytearray(256)
        self.used_words = 1

    def alloc(self, n_words: int) -> int:
        off = self.used_words
        self.used_words += n_words
        need = (self.used_words + 1) * WORD
        if len(self._buf) < need:
            self._buf.extend(b"\x00" * (need - len(self._buf)))
        return off

    def init_root(self, data_words: int, pointer_words: int) -> _CapnpStruct:
        off = self.alloc(data_words + pointer_words)
        self.write_struct_pointer(0, off, data_words, pointer_words)
        return _CapnpStruct(self, off, data_words, pointer_words)

    def finish(self) -> bytes:
        seg = bytes(self._buf[: self.used_words * WORD])
        header = struct.pack("<II", 0, self.used_words)
        return header + seg

    def write_struct_pointer(
        self, ptr_word: int, target_word: int, data_words: int, pointer_words: int
    ) -> None:
        offset = target_word - (ptr_word + 1)
        word = 0 | (offset << 2) | (data_words << 32) | (pointer_words << 48)
        self.set_word(ptr_word, word)

    def write_list_pointer(
        self, ptr_word: int, target_word: int, element_size: int, list_length: int
    ) -> None:
        offset = target_word - (ptr_word + 1)
        word = 1 | (offset << 2) | (element_size << 32) | (list_length << 35)
        self.set_word(ptr_word, word)

    def init_struct_list(
        self, ptr_word: int, count: int, data_words: int, pointer_words: int
    ) -> list[_CapnpStruct]:
        if count == 0:
            self.write_list_pointer(ptr_word, ptr_word + 1, 7, 0)
            return []
        elem_words = data_words + pointer_words
        payload_words = count * elem_words
        tag_word = self.alloc(1 + payload_words)
        self.write_list_pointer(ptr_word, tag_word, 7, payload_words)
        tag = 0 | (count << 2) | (data_words << 32) | (pointer_words << 48)
        self.set_word(tag_word, tag)
        return [
            _CapnpStruct(self, tag_word + 1 + i * elem_words, data_words, pointer_words)
            for i in range(count)
        ]

    def set_text(self, ptr_word: int, value: str) -> None:
        encoded = value.encode("utf-8") + b"\x00"
        self.set_byte_list(ptr_word, encoded)

    def set_data(self, ptr_word: int, value: bytes) -> None:
        self.set_byte_list(ptr_word, value)

    def set_byte_list(self, ptr_word: int, data: bytes) -> None:
        if not data:
            self.write_list_pointer(ptr_word, ptr_word + 1, 2, 0)
            return
        n_words = (len(data) + WORD - 1) // WORD
        target = self.alloc(n_words)
        start = target * WORD
        self._buf[start : start + len(data)] = data
        self.write_list_pointer(ptr_word, target, 2, len(data))

    def set_u16(self, word: int, field_index: int, value: int) -> None:
        struct.pack_into("<H", self._buf, word * WORD + field_index * 2, value & 0xFFFF)

    def set_u32(self, word: int, field_index: int, value: int) -> None:
        struct.pack_into("<I", self._buf, word * WORD + field_index * 4, value & 0xFFFFFFFF)

    def set_i64(self, word: int, field_index: int, value: int) -> None:
        struct.pack_into("<q", self._buf, word * WORD + field_index * 8, value)

    def set_u64(self, word: int, field_index: int, value: int) -> None:
        struct.pack_into("<Q", self._buf, word * WORD + field_index * 8, value & ((1 << 64) - 1))

    def set_f64(self, word: int, field_index: int, value: float) -> None:
        struct.pack_into("<d", self._buf, word * WORD + field_index * 8, value)

    def set_bool(self, word: int, bit_index: int, value: bool) -> None:
        byte_off = word * WORD + (bit_index >> 3)
        mask = 1 << (bit_index & 7)
        if value:
            self._buf[byte_off] |= mask
        else:
            self._buf[byte_off] &= ~mask

    def set_word(self, word: int, value: int) -> None:
        struct.pack_into("<Q", self._buf, word * WORD, value & ((1 << 64) - 1))


class _CapnpStruct:
    """Struct cursor on a :class:`_CapnpMessage`."""
    def __init__(
        self, msg: _CapnpMessage, word: int, data_words: int, pointer_words: int
    ) -> None:
        self.msg = msg
        self.word = word
        self.data_words = data_words
        self.pointer_words = pointer_words

    def pointer_word(self, index: int) -> int:
        return self.word + self.data_words + index

    def set_u16(self, field_index: int, value: int) -> None:
        self.msg.set_u16(self.word, field_index, value)

    def set_u32(self, field_index: int, value: int) -> None:
        self.msg.set_u32(self.word, field_index, value)

    def set_i64(self, field_index: int, value: int) -> None:
        self.msg.set_i64(self.word, field_index, value)

    def set_u64(self, field_index: int, value: int) -> None:
        self.msg.set_u64(self.word, field_index, value)

    def set_f64(self, field_index: int, value: float) -> None:
        self.msg.set_f64(self.word, field_index, value)

    def set_bool(self, bit_index: int, value: bool) -> None:
        self.msg.set_bool(self.word, bit_index, value)

    def set_text(self, pointer_index: int, value: str) -> None:
        self.msg.set_text(self.pointer_word(pointer_index), value)

    def set_data(self, pointer_index: int, value: bytes) -> None:
        self.msg.set_data(self.pointer_word(pointer_index), value)

    def init_struct_list(
        self, pointer_index: int, count: int, data_words: int, pointer_words: int
    ) -> list[_CapnpStruct]:
        return self.msg.init_struct_list(
            self.pointer_word(pointer_index), count, data_words, pointer_words
        )


class _CapnpReader:
    """Single-segment unpacked Cap'n reader."""

    _MAX_TRAVERSAL_WORDS = 64 * 1024

    def __init__(self, data: bytes) -> None:
        if len(data) < WORD:
            raise ValueError("truncated Cap'n message")
        nseg_minus, size0 = struct.unpack_from("<II", data, 0)
        nseg = nseg_minus + 1
        if nseg != 1:
            raise ValueError("multi-segment Cap'n messages are not supported")
        self._data = data
        self._seg = 8
        self._size0 = size0
        if self._seg + size0 * WORD > len(data):
            raise ValueError("truncated Cap'n segment")
        if size0 > self._MAX_TRAVERSAL_WORDS:
            raise ValueError("Cap'n segment exceeds traversal budget")

    def root(self, data_words: int, pointer_words: int) -> _StructReader:
        return self.struct_at(0, data_words, pointer_words)

    def struct_at(self, ptr_word: int, data_words: int, pointer_words: int) -> _StructReader:
        word = self.read_word(ptr_word)
        a = word & 3
        if a == 2 or a == 3:
            raise ValueError("far pointers are not supported")
        if a != 0:
            raise ValueError("expected struct pointer")
        offset = _sign_extend_30((word >> 2) & 0x3FFFFFFF)
        dw = (word >> 32) & 0xFFFF
        pw = (word >> 48) & 0xFFFF
        if dw < data_words or pw < pointer_words:
            raise ValueError("struct pointer smaller than expected")
        target = ptr_word + 1 + offset
        self._check_range(target, dw + pw)
        return _StructReader(self, target, dw, pw)

    def read_word(self, word: int) -> int:
        self._check_range(word, 1)
        return struct.unpack_from("<Q", self._data, self._seg + word * WORD)[0]

    def _check_range(self, word: int, n_words: int) -> None:
        if word < 0 or n_words < 0 or word + n_words > self._size0:
            raise ValueError("Cap'n pointer out of segment")

    def get_u16(self, word: int, field_index: int) -> int:
        return struct.unpack_from("<H", self._data, self._seg + word * WORD + field_index * 2)[0]

    def get_u32(self, word: int, field_index: int) -> int:
        return struct.unpack_from("<I", self._data, self._seg + word * WORD + field_index * 4)[0]

    def get_i64(self, word: int, field_index: int) -> int:
        return struct.unpack_from("<q", self._data, self._seg + word * WORD + field_index * 8)[0]

    def get_u64(self, word: int, field_index: int) -> int:
        return struct.unpack_from("<Q", self._data, self._seg + word * WORD + field_index * 8)[0]

    def get_f64(self, word: int, field_index: int) -> float:
        return struct.unpack_from("<d", self._data, self._seg + word * WORD + field_index * 8)[0]

    def get_bool(self, word: int, bit_index: int) -> bool:
        byte_off = self._seg + word * WORD + (bit_index >> 3)
        if byte_off < 0 or byte_off >= len(self._data):
            raise ValueError("Cap'n pointer out of segment")
        return (self._data[byte_off] & (1 << (bit_index & 7))) != 0

    def read_byte_list(self, ptr_word: int) -> bytes:
        word = self.read_word(ptr_word)
        if word == 0:
            return b""
        a = word & 3
        if a == 2 or a == 3:
            raise ValueError("far pointers are not supported")
        if a != 1:
            raise ValueError("expected list pointer")
        offset = _sign_extend_30((word >> 2) & 0x3FFFFFFF)
        c = (word >> 32) & 7
        d = word >> 35
        if c != 2:
            raise ValueError("expected byte list")
        target = ptr_word + 1 + offset
        n_words = (d + WORD - 1) // WORD if d else 0
        self._check_range(target, n_words)
        start = self._seg + target * WORD
        end = start + d
        if end > len(self._data):
            raise ValueError("truncated Cap'n byte list")
        return self._data[start:end]

    def read_text(self, ptr_word: int) -> str:
        data = self.read_byte_list(ptr_word)
        if data.endswith(b"\x00"):
            data = data[:-1]
        return data.decode("utf-8")

    def read_struct_list(
        self, ptr_word: int, data_words: int, pointer_words: int
    ) -> list[_StructReader]:
        word = self.read_word(ptr_word)
        if word == 0:
            return []
        a = word & 3
        if a == 2 or a == 3:
            raise ValueError("far pointers are not supported")
        if a != 1:
            raise ValueError("expected list pointer")
        offset = _sign_extend_30((word >> 2) & 0x3FFFFFFF)
        c = (word >> 32) & 7
        d = word >> 35
        if c != 7:
            raise ValueError("expected composite list")
        if d == 0:
            return []
        tag_word = ptr_word + 1 + offset
        self._check_range(tag_word, 1)
        tag = self.read_word(tag_word)
        count = (tag >> 2) & 0x3FFFFFFF
        dw = (tag >> 32) & 0xFFFF
        pw = (tag >> 48) & 0xFFFF
        if dw < data_words or pw < pointer_words:
            raise ValueError("composite element smaller than expected")
        elem_words = dw + pw
        if elem_words and count > d // elem_words:
            raise ValueError("composite list count exceeds payload")
        if count > self._MAX_TRAVERSAL_WORDS:
            raise ValueError("composite list count exceeds traversal budget")
        self._check_range(tag_word, 1 + count * elem_words)
        return [
            _StructReader(self, tag_word + 1 + i * elem_words, dw, pw) for i in range(count)
        ]


def _sign_extend_30(n: int) -> int:
    n &= 0x3FFFFFFF
    if n & 0x20000000:
        return n - 0x40000000
    return n


class _StructReader:
    """Struct cursor on a :class:`_CapnpReader`."""
    def __init__(
        self, reader: _CapnpReader, word: int, data_words: int, pointer_words: int
    ) -> None:
        self.reader = reader
        self.word = word
        self.data_words = data_words
        self.pointer_words = pointer_words

    def pointer_word(self, index: int) -> int:
        return self.word + self.data_words + index

    def get_u16(self, field_index: int) -> int:
        return self.reader.get_u16(self.word, field_index)

    def get_u32(self, field_index: int) -> int:
        return self.reader.get_u32(self.word, field_index)

    def get_i64(self, field_index: int) -> int:
        return self.reader.get_i64(self.word, field_index)

    def get_u64(self, field_index: int) -> int:
        return self.reader.get_u64(self.word, field_index)

    def get_f64(self, field_index: int) -> float:
        return self.reader.get_f64(self.word, field_index)

    def get_bool(self, bit_index: int) -> bool:
        return self.reader.get_bool(self.word, bit_index)

    def get_text(self, pointer_index: int) -> str:
        return self.reader.read_text(self.pointer_word(pointer_index))

    def get_data(self, pointer_index: int) -> bytes:
        return self.reader.read_byte_list(self.pointer_word(pointer_index))

    def get_struct_list(
        self, pointer_index: int, data_words: int, pointer_words: int
    ) -> list[_StructReader]:
        return self.reader.read_struct_list(
            self.pointer_word(pointer_index), data_words, pointer_words
        )


def _write_db_value(root: _CapnpStruct, value: DbValue) -> None:
    kind = value["kind"]
    if kind == "null":
        root.set_u16(0, _DB_TYPE_ORD[value["value"]])
        root.set_u16(1, 0)
        return
    if kind == "boolean":
        root.set_bool(0, value["value"])
        root.set_u16(1, 1)
        return
    if kind == "int64":
        n = value["value"]
        if n < I64_MIN or n > I64_MAX:
            raise ValueError("int64 DbValue is out of range")
        root.set_i64(1, n)
        root.set_u16(1, 2)
        return
    if kind == "float64":
        n = value["value"]
        if not math.isfinite(n):
            raise ValueError("float64 value is not finite")
        root.set_f64(1, n)
        root.set_u16(1, 3)
        return
    if kind == "text":
        root.set_u16(1, 4)
        root.set_text(0, value["value"])
        return
    if kind == "bytes":
        root.set_u16(1, 5)
        root.set_data(0, value["value"])
        return
    raise ValueError(f"unknown DbValue union member: {kind}")


def _read_db_value(root: _StructReader) -> DbValue:
    disc = root.get_u16(1)
    if disc == 0:
        ty = _DB_TYPE_FROM[root.get_u16(0)]
        return {"kind": "null", "value": ty}
    if disc == 1:
        return {"kind": "boolean", "value": root.get_bool(0)}
    if disc == 2:
        return {"kind": "int64", "value": root.get_i64(1)}
    if disc == 3:
        n = root.get_f64(1)
        if not math.isfinite(n):
            raise ValueError("float64 value is not finite")
        return {"kind": "float64", "value": n}
    if disc == 4:
        return {"kind": "text", "value": root.get_text(0)}
    if disc == 5:
        return {"kind": "bytes", "value": root.get_data(0)}
    raise ValueError(f"unknown DbValue union member: {disc}")


def _write_statement(s: _CapnpStruct, stmt: TypedDbStatement) -> None:
    s.set_text(0, stmt["sql"])
    s.set_u16(0, _KIND_ORD[stmt["kind"]])
    s.set_u16(1, _SELECT_ORD[stmt["resultSelection"]])
    s.set_u32(1, stmt["maxRows"])
    params = s.init_struct_list(1, len(stmt["parameters"]), 2, 1)
    for i, param in enumerate(stmt["parameters"]):
        _write_db_value(params[i], param)


def _read_statement(s: _StructReader) -> TypedDbStatement:
    kind = _KIND_FROM[s.get_u16(0)]
    selection = _SELECT_FROM[s.get_u16(1)]
    return {
        "sql": s.get_text(0),
        "parameters": [_read_db_value(p) for p in s.get_struct_list(1, 2, 1)],
        "kind": kind,
        "maxRows": s.get_u32(1),
        "resultSelection": selection,
    }
