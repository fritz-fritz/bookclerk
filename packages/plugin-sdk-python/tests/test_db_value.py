"""Golden vectors for typed DbValue (nulls, i64 bounds, UTF-8, unknown unions)."""

from __future__ import annotations

import unittest

from bookclerk_plugin_sdk.db_value import parse_db_value


class DbValueGoldens(unittest.TestCase):
    def test_typed_null_bytes(self) -> None:
        self.assertEqual(
            parse_db_value({"kind": "null", "value": "bytes"}),
            {"kind": "null", "value": "bytes"},
        )

    def test_i64_min_max(self) -> None:
        for n in (-(2**63), -1, 0, 1, 2**63 - 1):
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

    def test_unknown_union_member(self) -> None:
        with self.assertRaisesRegex(ValueError, "unknown DbValue union member"):
            parse_db_value({"kind": "xml", "value": "<a/>"})


if __name__ == "__main__":
    unittest.main()
