//! Public `database_adapter` surface guards.

#[cfg(test)]
mod tests {
    /// Ensures host-only session helpers stay off the stable author surface.
    ///
    /// # Panics
    ///
    /// Panics when `database_adapter.rs` publicly exports a host-session helper
    /// or declares `pub mod session` / `pub mod host_session`.
    #[test]
    fn public_database_adapter_does_not_export_host_session_helpers() {
        let src = include_str!("../database_adapter.rs");
        for forbidden in ["pub mod session", "pub mod host_session", "pub mod sql"] {
            assert!(
                !src.contains(forbidden),
                "database_adapter must not declare `{forbidden}`"
            );
        }
        for forbidden in [
            "guest_query",
            "guest_execute",
            "guest_atomic",
            "guest_begin",
            "guest_query_page",
            "row_to_dto",
        ] {
            assert!(
                !src.contains(&format!("pub use session::{forbidden}")),
                "database_adapter must not publicly re-export `{forbidden}`"
            );
        }
    }
}
