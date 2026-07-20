//! `libation copydb` — export library.db to PostgreSQL (LibationCli: `copydb`).

use std::path::PathBuf;

use clap::Args;
use libation_config::Config;
use rusqlite::Connection;

#[derive(Debug, Args)]
pub struct CopyDbArgs {
    /// PostgreSQL connection string.
    #[arg(short = 'c', long)]
    connection: String,
    /// Source SQLite path (default: `{files_dir}/library.db`).
    #[arg(long)]
    source: Option<PathBuf>,
}

pub async fn run(args: CopyDbArgs, config: &Config) -> anyhow::Result<()> {
    let paths = config.paths();
    let source = args
        .source
        .unwrap_or_else(|| paths.library_db.clone());
    let conn = Connection::open_with_flags(
        &source,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    let (mut client, connection) =
        tokio_postgres::connect(&args.connection, tokio_postgres::NoTls).await?;
    tokio::spawn(async move {
        if let Err(err) = connection.await {
            tracing::error!(error = %err, "postgres connection error");
        }
    });

    client.batch_execute(
        r#"
        CREATE TABLE IF NOT EXISTS accounts (
            id SERIAL PRIMARY KEY,
            account_id TEXT NOT NULL UNIQUE,
            marketplace TEXT NOT NULL,
            label TEXT,
            scan_enabled BOOLEAN NOT NULL DEFAULT TRUE,
            created_at TIMESTAMPTZ NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL
        );
        CREATE TABLE IF NOT EXISTS books (
            id SERIAL PRIMARY KEY,
            asin TEXT NOT NULL,
            account_id TEXT NOT NULL,
            marketplace TEXT NOT NULL,
            title TEXT NOT NULL,
            authors TEXT,
            narrators TEXT,
            series TEXT,
            series_index TEXT,
            liberate_status TEXT NOT NULL,
            storage_key TEXT,
            error_message TEXT,
            purchased_at TIMESTAMPTZ,
            tags TEXT,
            rating_overall REAL,
            rating_performance REAL,
            rating_story REAL,
            is_finished BOOLEAN NOT NULL DEFAULT FALSE,
            pdf_status TEXT NOT NULL,
            pdf_storage_key TEXT,
            publisher TEXT,
            length_minutes INTEGER,
            is_abridged BOOLEAN NOT NULL DEFAULT FALSE,
            content_kind TEXT NOT NULL DEFAULT 'book',
            categories TEXT,
            subtitle TEXT,
            published_at TIMESTAMPTZ,
            created_at TIMESTAMPTZ NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL,
            UNIQUE (asin, account_id)
        );
        CREATE TABLE IF NOT EXISTS saved_filters (
            id SERIAL PRIMARY KEY,
            name TEXT NOT NULL UNIQUE,
            query TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL
        );
        "#,
    )
    .await?;

    let tx = client.transaction().await?;
    tx.execute("DELETE FROM books", &[]).await?;
    tx.execute("DELETE FROM accounts", &[]).await?;
    tx.execute("DELETE FROM saved_filters", &[]).await?;

    let mut stmt = conn.prepare(
        "SELECT account_id, marketplace, label, scan_enabled, created_at, updated_at FROM accounts",
    )?;
    let accounts = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, i64>(3)? != 0,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;
    let mut acct_count = 0usize;
    for row in accounts {
        let (account_id, marketplace, label, scan_enabled, created_at, updated_at) = row?;
        tx.execute(
            "INSERT INTO accounts (account_id, marketplace, label, scan_enabled, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5::timestamptz, $6::timestamptz)",
            &[
                &account_id,
                &marketplace,
                &label,
                &scan_enabled,
                &created_at,
                &updated_at,
            ],
        )
        .await?;
        acct_count += 1;
    }

    let mut stmt = conn.prepare(
        "SELECT asin, account_id, marketplace, title, authors, narrators, series, series_index,
                liberate_status, storage_key, error_message, purchased_at, tags,
                rating_overall, rating_performance, rating_story, is_finished,
                pdf_status, pdf_storage_key, publisher, length_minutes, is_abridged,
                content_kind, categories, subtitle, published_at, created_at, updated_at
         FROM books",
    )?;
    let books = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<String>>(10)?,
            row.get::<_, Option<String>>(11)?,
            row.get::<_, Option<String>>(12)?,
            row.get::<_, Option<f64>>(13)?,
            row.get::<_, Option<f64>>(14)?,
            row.get::<_, Option<f64>>(15)?,
            row.get::<_, i64>(16)? != 0,
            row.get::<_, String>(17)?,
            row.get::<_, Option<String>>(18)?,
            row.get::<_, Option<String>>(19)?,
            row.get::<_, Option<i64>>(20)?,
            row.get::<_, i64>(21)? != 0,
            row.get::<_, String>(22).unwrap_or_else(|_| "book".into()),
            row.get::<_, Option<String>>(23).ok().flatten(),
            row.get::<_, Option<String>>(24).ok().flatten(),
            row.get::<_, Option<String>>(25).ok().flatten(),
            row.get::<_, String>(26)?,
            row.get::<_, String>(27)?,
        ))
    })?;
    let mut book_count = 0usize;
    for row in books {
        let (
            asin,
            account_id,
            marketplace,
            title,
            authors,
            narrators,
            series,
            series_index,
            liberate_status,
            storage_key,
            error_message,
            purchased_at,
            tags,
            rating_overall,
            rating_performance,
            rating_story,
            is_finished,
            pdf_status,
            pdf_storage_key,
            publisher,
            length_minutes,
            is_abridged,
            content_kind,
            categories,
            subtitle,
            published_at,
            created_at,
            updated_at,
        ) = row?;
        tx.execute(
            "INSERT INTO books (
                asin, account_id, marketplace, title, authors, narrators, series, series_index,
                liberate_status, storage_key, error_message, purchased_at, tags,
                rating_overall, rating_performance, rating_story, is_finished,
                pdf_status, pdf_storage_key, publisher, length_minutes, is_abridged,
                content_kind, categories, subtitle, published_at, created_at, updated_at
            ) VALUES (
                $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12::timestamptz,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27::timestamptz,$28::timestamptz
            )",
            &[
                &asin,
                &account_id,
                &marketplace,
                &title,
                &authors,
                &narrators,
                &series,
                &series_index,
                &liberate_status,
                &storage_key,
                &error_message,
                &purchased_at,
                &tags,
                &rating_overall,
                &rating_performance,
                &rating_story,
                &is_finished,
                &pdf_status,
                &pdf_storage_key,
                &publisher,
                &length_minutes,
                &is_abridged,
                &content_kind,
                &categories,
                &subtitle,
                &published_at,
                &created_at,
                &updated_at,
            ],
        )
        .await?;
        book_count += 1;
    }

    tx.commit().await?;
    println!(
        "copied {acct_count} account(s) and {book_count} book(s) from {} to postgres",
        source.display()
    );
    Ok(())
}
