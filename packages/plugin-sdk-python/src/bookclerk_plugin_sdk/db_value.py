"""Universal Cap'n database value domain (``DbValue``).

Baseline cells are typed null, bool, int64, finite float64, UTF-8 text, and
bytes. Unknown ``kind`` values fail closed.
"""

from __future__ import annotations

from typing import Any, Literal, TypedDict, Union

DbType = Literal["unspecified", "bool", "int64", "float64", "text", "bytes"]

KINDS = frozenset({"null", "boolean", "int64", "float64", "text", "bytes"})
TYPES = frozenset({"unspecified", "bool", "int64", "float64", "text", "bytes"})


class NullValue(TypedDict):
    """Typed SQL NULL with a declared ``DbType``."""

    kind: Literal["null"]
    value: DbType


class BoolValue(TypedDict):
    """Boolean cell."""

    kind: Literal["boolean"]
    value: bool


class Int64Value(TypedDict):
    """Signed 64-bit integer cell."""

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
    """Binary cell encoded as a ``b64:`` string."""

    kind: Literal["bytes"]
    value: str


DbValue = Union[NullValue, BoolValue, Int64Value, Float64Value, TextValue, BytesValue]


def parse_db_value(raw: Any) -> DbValue:
    """Parse a JSON ``DbValue``. Unknown union members raise ``ValueError``."""
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
        if not isinstance(value, str):
            raise ValueError("bytes DbValue requires a string")
        return {"kind": "bytes", "value": value}
    raise ValueError(f"unknown DbValue union member: {kind}")
