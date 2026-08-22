"""Golden vectors for typed DbValue (Cap'n wire, i64 bounds, UTF-8, unknown unions)."""

from __future__ import annotations

import unittest

from bookclerk_plugin_sdk.db_value import (
    DatabaseBinding,
    decode_db_value,
    decode_execute_request,
    encode_db_value,
    encode_execute_request,
    parse_db_value,
)

I64_MIN = -(2**63)
I64_MAX = 2**63 - 1

GOLDEN_I64_MIN = (
    "00000000040000000000000002000100000002000000000000000000000000800000000000000000"
)
GOLDEN_I64_MAX = (
    "000000000400000000000000020001000000020000000000ffffffffffffff7f0000000000000000"
)
GOLDEN_TEXT_B64 = (
    "0000000006000000000000000200010000000400000000000000000000000000010000004a000000"
    "6236343a414141410000000000000000"
)
GOLDEN_BYTES_012 = (
    "0000000005000000000000000200010000000500000000000000000000000000010000001a000000"
    "0001020000000000"
)
GOLDEN_BOOL_TRUE = (
    "00000000040000000000000002000100010001000000000000000000000000000000000000000000"
)
GOLDEN_NULL_BYTES = (
    "00000000040000000000000002000100050000000000000000000000000000000000000000000000"
)
GOLDEN_EXECUTE_REQUEST = (
    "000000001d0000000000000004000300000000000000000000000000000000000000000000000000"
    "0000000000000000090000001a0000000900000022000000090000001f0000006f70000000000000"
    "616263000000000004000000010002000200020001000000050000004a000000090000004f000000"
    "53454c454354203f00000000000000000c0000000200010000000200000000000000000000000080"
    "0000000000000000000004000000000000000000000000000d000000720000000000050000000000"
    "0000000000000000090000000a0000006236343a6e6f742d6279746573000000ff00000000000000"
)


class DbValueGoldens(unittest.TestCase):
    def test_typed_null_bytes(self) -> None:
        self.assertEqual(
            parse_db_value({"kind": "null", "value": "bytes"}),
            {"kind": "null", "value": "bytes"},
        )

    def test_i64_min_max(self) -> None:
        for n in (I64_MIN, -1, 0, 1, I64_MAX):
            self.assertEqual(
                parse_db_value({"kind": "int64", "value": n}),
                {"kind": "int64", "value": n},
            )

    def test_utf8_and_embedded_nul(self) -> None:
        text = parse_db_value({"kind": "text", "value": "héllo\x00world"})
        self.assertEqual(text["value"], "héllo\x00world")

    def test_embedded_zero_bytes(self) -> None:
        blob = parse_db_value({"kind": "bytes", "value": "b64:AAEC"})
        self.assertEqual(blob["kind"], "bytes")
        self.assertEqual(blob["value"], b"\x00\x01\x02")
        raw = parse_db_value({"kind": "bytes", "value": b"\x00\x01\x02"})
        self.assertEqual(raw["value"], b"\x00\x01\x02")

    def test_unknown_union_member(self) -> None:
        with self.assertRaisesRegex(ValueError, "unknown DbValue union member"):
            parse_db_value({"kind": "xml", "value": "<a/>"})

    def test_capnp_goldens(self) -> None:
        self.assertEqual(encode_db_value({"kind": "int64", "value": I64_MIN}).hex(), GOLDEN_I64_MIN)
        self.assertEqual(encode_db_value({"kind": "int64", "value": I64_MAX}).hex(), GOLDEN_I64_MAX)
        self.assertEqual(
            encode_db_value({"kind": "text", "value": "b64:AAAA"}).hex(), GOLDEN_TEXT_B64
        )
        self.assertEqual(
            encode_db_value({"kind": "bytes", "value": b"\x00\x01\x02"}).hex(),
            GOLDEN_BYTES_012,
        )
        self.assertEqual(encode_db_value({"kind": "boolean", "value": True}).hex(), GOLDEN_BOOL_TRUE)
        self.assertEqual(
            encode_db_value({"kind": "null", "value": "bytes"}).hex(), GOLDEN_NULL_BYTES
        )
        self.assertNotEqual(
            encode_db_value({"kind": "text", "value": "b64:AAAA"}),
            encode_db_value({"kind": "bytes", "value": b"\x00\x01\x02"}),
        )
        self.assertEqual(decode_db_value(encode_db_value({"kind": "int64", "value": I64_MIN}))["value"], I64_MIN)
        self.assertEqual(
            decode_db_value(encode_db_value({"kind": "text", "value": "b64:AAAA"}))["value"],
            "b64:AAAA",
        )
        self.assertEqual(
            decode_db_value(encode_db_value({"kind": "bytes", "value": b"\x00\x01\x02"}))["value"],
            b"\x00\x01\x02",
        )

    def test_execute_request_golden(self) -> None:
        request = {
            "operationId": "op",
            "requestHash": "abc",
            "statements": [
                {
                    "sql": "SELECT ?",
                    "parameters": [
                        {"kind": "int64", "value": I64_MIN},
                        {"kind": "text", "value": "b64:not-bytes"},
                        {"kind": "bytes", "value": b"\xff"},
                    ],
                    "kind": "select",
                    "maxRows": 1,
                    "resultSelection": "rows",
                }
            ],
            "outcomeIndex": 0,
            "payloadIndex": 0,
            "hasPayloadIndex": False,
            "priorReceiptIndex": 0,
            "hasPriorReceiptIndex": False,
            "receiptSelectIndex": 0,
            "hasReceiptSelectIndex": False,
            "deadlineUnixMs": 0,
        }
        encoded = encode_execute_request(request)
        self.assertEqual(encoded.hex(), GOLDEN_EXECUTE_REQUEST)
        back = decode_execute_request(encoded)
        self.assertEqual(back["operationId"], "op")
        self.assertEqual(back["statements"][0]["parameters"][0]["value"], I64_MIN)
        self.assertEqual(back["statements"][0]["parameters"][1]["value"], "b64:not-bytes")
        self.assertEqual(back["statements"][0]["parameters"][2]["value"], b"\xff")

        seen = {"n": 0}

        def transport(req):
            seen["n"] = len(encode_execute_request(req))
            return {
                "operationId": req["operationId"],
                "statements": [],
                "timing": {
                    "attemptElapsedUs": 0,
                    "dbExecutionUs": 0,
                    "dbTimingSource": "test",
                },
            }

        binding = DatabaseBinding(transport, operation_id="op", request_hash="abc")
        reply = binding.execute(request["statements"])
        self.assertEqual(reply["operationId"], "op")
        self.assertEqual(seen["n"], len(encoded))


if __name__ == "__main__":
    unittest.main()
