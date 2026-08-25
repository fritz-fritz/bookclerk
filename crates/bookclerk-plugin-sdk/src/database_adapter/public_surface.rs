//! Public `database_adapter` surface guards.

#[cfg(test)]
mod tests {
    #[test]
    fn public_database_adapter_does_not_export_host_session_helpers() {
        let src = include_str!("../database_adapter.rs");
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
                "database_adapter must not publicly export `{forbidden}`"
            );
        }
    }
}
