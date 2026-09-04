//! Shared SQL-contract constants.
//!
//! Typed vectors live in [`super::typed_vectors`] / [`super::vectors_typed`].

/// Injected `maxResultRows` for conn-vector row-cap cases (sqlite / postgres).
pub const CONTRACT_VECTOR_ROW_CAP: u32 = 5;

/// One ~160 KiB TEXT cell without embedding the payload in statement text.
///
/// D1 `maxPayloadBytes` and the 100 KiB physical SQL limit cannot admit a
/// 150 KiB literal. Doubling CTE: `10 * 2^14 = 163840` bytes. Two such
/// statements exceed [`bookclerk_plugin_abi::FIRST_PARTY_MAX_RESULT_BYTES`].
pub(super) const LARGE_RESULT_PAD_SQL: &str = "WITH RECURSIVE t(n, s) AS (\
 SELECT 1, 'aaaaaaaaaa' \
 UNION ALL \
 SELECT n + 1, s || s FROM t WHERE n < 15\
) SELECT s AS pad FROM t WHERE n = 15";
