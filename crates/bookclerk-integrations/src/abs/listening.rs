//! Sync AudioBookshelf media progress into `listening_progress`.

use bookclerk_library::{LibraryStore, NewListeningProgress};
use chrono::{TimeZone, Utc};

use crate::abs::client::AbsApiClient;
use crate::error::Result;

const PROVIDER: &str = "audiobookshelf";

/// Pull ABS user media progress into the library DB.
///
/// Best-effort matches rows to `book_uuid` / `work_id` via ASIN, ISBN, or title.
pub async fn sync_listening_progress(
    library: &LibraryStore,
    client: &AbsApiClient,
) -> Result<usize> {
    let users = client.list_users().await?;
    let mut upserted = 0usize;

    for user in users {
        let detail = match client.get_user(&user.id).await {
            Ok(d) => d,
            Err(err) => {
                tracing::warn!(user_id = %user.id, error = %err, "ABS get_user failed");
                continue;
            }
        };

        let identity_id = library
            .get_portal_identity(PROVIDER, &detail.id)
            .ok()
            .flatten()
            .map(|i| i.id);

        for prog in detail.media_progress {
            let mut title = None;
            let mut authors = None;
            let mut asin = None;
            let mut isbn = None;

            match client.get_library_item(&prog.library_item_id).await {
                Ok(item) => {
                    if let Some(meta) = item.media.and_then(|m| m.metadata) {
                        title = meta.title;
                        authors = meta.author_name;
                        asin = meta.asin.map(|s| s.to_ascii_uppercase());
                        isbn = meta.isbn;
                    }
                }
                Err(err) => {
                    tracing::debug!(
                        item_id = %prog.library_item_id,
                        error = %err,
                        "ABS get_library_item failed"
                    );
                }
            }

            let book_uuid =
                match_book_uuid(library, asin.as_deref(), isbn.as_deref(), title.as_deref())?;
            let work_id = if let Some(ref uuid) = book_uuid {
                library.work_id_for_book(uuid).ok().flatten()
            } else {
                None
            };

            let last_listened_at = prog
                .last_update
                .and_then(|ms| Utc.timestamp_millis_opt(ms).single());

            library.upsert_listening_progress(&NewListeningProgress {
                identity_id,
                provider: PROVIDER.into(),
                external_user_id: detail.id.clone(),
                book_uuid,
                work_id,
                external_item_id: prog.library_item_id.clone(),
                title,
                authors,
                asin,
                isbn,
                progress: prog.progress,
                current_time_seconds: prog.current_time,
                duration_seconds: prog.duration,
                is_finished: prog.is_finished,
                last_listened_at,
            })?;
            upserted += 1;
        }
    }

    Ok(upserted)
}

fn match_book_uuid(
    library: &LibraryStore,
    asin: Option<&str>,
    isbn: Option<&str>,
    title: Option<&str>,
) -> Result<Option<String>> {
    let books = library.list_books(None)?;
    if let Some(asin) = asin {
        if let Some(b) = books.iter().find(|b| {
            b.asin
                .as_deref()
                .map(|a| a.eq_ignore_ascii_case(asin))
                .unwrap_or(false)
        }) {
            return Ok(Some(b.uuid.clone()));
        }
    }
    if let Some(isbn) = isbn {
        if let Some(b) = books.iter().find(|b| b.isbn.as_deref() == Some(isbn)) {
            return Ok(Some(b.uuid.clone()));
        }
    }
    if let Some(title) = title {
        let title_l = title.to_lowercase();
        if let Some(b) = books.iter().find(|b| b.title.to_lowercase() == title_l) {
            return Ok(Some(b.uuid.clone()));
        }
    }
    Ok(None)
}
