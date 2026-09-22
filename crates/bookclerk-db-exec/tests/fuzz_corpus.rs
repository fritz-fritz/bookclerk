//! Replay checked-in cargo-fuzz corpora in PR CI (no libFuzzer required).

use std::path::{Path, PathBuf};

use sea_orm::DatabaseBackend;

/// Canonical containment under the corpus root (CodeQL two-state barrier).
fn under_corpus(root: &Path, path: PathBuf) -> PathBuf {
    let root = root.canonicalize().expect("canonicalize corpus root");
    let path = if path.exists() {
        path.canonicalize().expect("canonicalize corpus entry")
    } else {
        path
    };
    assert!(
        path.starts_with(&root),
        "corpus path {} escapes {}",
        path.display(),
        root.display()
    );
    path
}

#[test]
fn fuzz_corpus_sql_lower_does_not_panic() {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/sql_lower");
    let dir = dir.canonicalize().expect("canonicalize fuzz corpus");
    for entry in std::fs::read_dir(&dir).expect("fuzz corpus") {
        let path = under_corpus(&dir, entry.expect("entry").path());
        if !path.is_file() {
            continue;
        }
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
