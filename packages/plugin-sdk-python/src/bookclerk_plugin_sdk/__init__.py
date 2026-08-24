"""Bookclerk Python plugin guest SDK.

Provides the workerd guest surface via :mod:`bookclerk_plugin_sdk.workerd`.
Authors subclass :class:`BookclerkPlugin` and export the raw class. Native
guests use the Rust SDK (`serve` / `PluginRoot`). See ``docs/plugins.md``.

Typical import:

- Workerd: ``from bookclerk_plugin_sdk.workerd import BookclerkPlugin, js``
"""

from .abi import (
    API_VERSION,
    METHOD_NAMES,
    BrandDto,
    CliInvokeParams,
    CliInvokeResult,
    CliSchema,
    FetchTitleParams,
    HandshakeParams,
    HandshakeResult,
    HealthResult,
    LoginParams,
    PluginError,
    ScanParams,
    StatementDto,
)
from .db_value import (
    DatabaseBinding,
    D1ExecResult,
    D1Meta,
    D1Result,
    RetryToken,
    create_database_binding,
    canonical_execute_request_hash,
    decode_db_value,
    decode_execute_result_reply,
    decode_execute_request,
    encode_db_value,
    encode_execute_result_reply,
    encode_execute_request,
    execute_reply_to_d1_results,
    parse_db_value,
    statement_result_to_d1_result,
)
from .native import BookclerkPlugin, BookclerkPluginGuest

__all__ = [
    "API_VERSION",
    "METHOD_NAMES",
    "BrandDto",
    "BookclerkPlugin",
    "BookclerkPluginGuest",
    "CliInvokeParams",
    "CliInvokeResult",
    "CliSchema",
    "DatabaseBinding",
    "D1ExecResult",
    "D1Meta",
    "D1Result",
    "RetryToken",
    "create_database_binding",
    "canonical_execute_request_hash",
    "decode_db_value",
    "decode_execute_result_reply",
    "decode_execute_request",
    "encode_db_value",
    "encode_execute_result_reply",
    "encode_execute_request",
    "FetchTitleParams",
    "HandshakeParams",
    "HandshakeResult",
    "HealthResult",
    "LoginParams",
    "parse_db_value",
    "PluginError",
    "ScanParams",
    "StatementDto",
]
