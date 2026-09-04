//! Shared SQL-contract constants.
//!
//! Typed vectors live in [`super::typed_vectors`] / [`super::vectors_typed`].

/// Injected `maxResultRows` for conn-vector row-cap cases (sqlite / postgres).
pub const CONTRACT_VECTOR_ROW_CAP: u32 = 5;
