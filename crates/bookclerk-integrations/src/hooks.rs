//! Helpers to persist acquire success as a durable `book_acquired` outbox event.

use bookclerk_library::{LibraryStore, PublishDomainEventSpec};
use tracing::debug;

/// Look up the acquired book and publish `book_acquired` to the durable outbox.
///
/// Delivery is handled by the daemon event worker, not by in-process fan-out.
pub async fn emit_book_acquired(library: &LibraryStore, product_or_uuid: &str, storage_key: &str) {
    let mut book = library
        .get_book_by_uuid(product_or_uuid)
        .await
        .ok()
        .flatten();
    if book.is_none() {
        book = library
            .list_books(None)
            .await
            .ok()
            .into_iter()
            .flatten()
            .find(|b| {
                b.uuid == product_or_uuid
                    || b.product_id == product_or_uuid
                    || b.asin.as_deref() == Some(product_or_uuid)
                    || b.storage_key.as_deref() == Some(storage_key)
            });
    }

    let Some(book) = book else {
        debug!(id = %product_or_uuid, "no book row for acquire event");
        return;
    };

    let payload = serde_json::json!({
        "titleId": book.uuid,
        "source": book.source,
        "asin": book.asin,
        "isbn": book.isbn,
        "pathKeys": [storage_key],
        "accountId": book.account_id,
    });
    let payload = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into());
    let spec = PublishDomainEventSpec {
        id: String::new(),
        event_type: "book_acquired".into(),
        schema_version: 1,
        account_id: book.account_id.clone(),
        correlation_id: book.uuid.clone(),
        causation_id: String::new(),
        dedup_key: format!("book_acquired:{}", book.uuid),
        payload,
        ordering_key: book.uuid.clone(),
    };
    match library.publish_domain_event(spec).await {
        Ok(_) => {}
        Err(err) => {
            tracing::warn!(error = %err, uuid = %book.uuid, "failed to publish book_acquired");
        }
    }
}
