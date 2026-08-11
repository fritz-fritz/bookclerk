"""ABI constants — keep aligned with ``crates/bookclerk-plugin-abi/schema/abi.json``.

Machine-facing method names and the negotiated ``api_version`` shared by
native stdio guests and workerd Python Workers. Regenerated projections in
other languages consume the same schema; do not rename entries here without
updating the ABI crate.
"""

from __future__ import annotations

API_VERSION: int = 1
"""Negotiated Bookclerk plugin ABI version (must match ``plugin.toml``)."""

METHOD_NAMES: tuple[str, ...] = (
    "handshake",
    "shutdown",
    "health",
    "diagnose",
    "start",
    "onEvent",
    "pollEvents",
    "scanLibrary",
    "syncListening",
    "authenticateUser",
    "cliDescribe",
    "cliInvoke",
    "login",
    "loginStart",
    "loginComplete",
    "credentialsUpdate",
    "scan",
    "fetchTitle",
    "searchCatalog",
    "expandCandidates",
    "purchaseHint",
    "listDeals",
    "listAccounts",
    "catalogDetail",
    "put",
    "putFile",
    "get",
    "exists",
    "list",
    "probe",
    "copy",
    "delete",
    "touchFile",
    "dbConnect",
    "dbPing",
    "dbQuery",
    "dbExecute",
)
"""Canonical Workers RPC method names exposed on the guest surface."""
