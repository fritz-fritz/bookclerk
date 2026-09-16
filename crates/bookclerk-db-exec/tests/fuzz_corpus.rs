//! Replay checked-in cargo-fuzz corpora in PR CI (no libFuzzer required).

use sea_orm::DatabaseBackend;
use std::path::{Path, PathBuf};

fn rebuild_test_path(path: &Path) -> PathBuf {
    let s = path.to_string_lossy().into_owned();
    assert!(!s.contains("..") && !s.contains('\0'));
    PathBuf::from(s)
}

#[test]
fn fuzz_corpus_sql_lower_does_not_panic() {
    let dir = rebuild_test_path(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/sql_lower"),
    );
    // codeql[rust/path-injection]
    for entry in std::fs::read_dir(&dir).expect("fuzz corpus") {
        let path = rebuild_test_path(&entry.expect("entry").path());
        // codeql[rust/path-injection]
        if !path.is_file() {
            continue;
        }
        // codeql[rust/path-injection]
        let sql = std::fs::read_to_string(&path).expect("read");
        let sqlite = bookclerk_db_exec::lower_canonical_sql(DatabaseBackend::Sqlite, &sql);
        let postgres = bookclerk_db_exec::lower_canonical_sql(DatabaseBackend::Postgres, &sql);
        if sql.contains(';') && sql.contains('\'') {
            assert!(
                sqlite.contains(';') && postgres.contains(';'),
                "{} lost semicolon: {sqlite} / {postgres}",
                path.display()
            );
        }
        bookclerk_db_exec::lower_canonical_sql_typed(DatabaseBackend::Sqlite, &sql, None).ok();
        bookclerk_db_exec::lower_canonical_sql_typed(DatabaseBackend::Postgres, &sql, None).ok();
    }
}
