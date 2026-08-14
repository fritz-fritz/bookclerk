//! Fail-closed helpers for the temporary ABI v1 JSON destination adapter.
//!
//! Oversized scalar `put` / `get` must not silently buffer media. Stream methods
//! on v1 guests either stay under [`MAX_SCALAR_BYTES`] or return
//! [`StorageError::PayloadTooLarge`].

#![allow(clippy::missing_docs_in_private_items)]

use std::pin::Pin;

use bookclerk_plugin_sdk::v2::MAX_SCALAR_BYTES;
use bookclerk_storage::{
    ByteRange, ListPage, ObjectInfo, ObjectProbe, PutStreamResult, StorageError,
};
use bytes::Bytes;
use tokio::io::{AsyncRead, AsyncReadExt};

/// Rejects a scalar payload that exceeds the v1 fail-closed cap.
///
/// # Errors
///
/// Returns [`StorageError::PayloadTooLarge`] when `len` is above the cap.
pub(crate) fn reject_oversize_scalar(len: u64, op: &str) -> bookclerk_storage::Result<()> {
    if len > u64::from(MAX_SCALAR_BYTES) {
        return Err(StorageError::PayloadTooLarge(format!(
            "v1 adapter {op} of {len} bytes exceeds {MAX_SCALAR_BYTES} (use api_version 2 streams)"
        )));
    }
    Ok(())
}

/// Paginates an already-fetched listing (v1 guests have no list-page RPC).
#[must_use]
pub(crate) fn paginate_objects(all: Vec<ObjectInfo>, cursor: Option<&str>, limit: u32) -> ListPage {
    let start = cursor
        .and_then(|c| all.iter().position(|o| o.key.as_str() == c).map(|i| i + 1))
        .unwrap_or(0);
    let limit = if limit == 0 { 256 } else { limit as usize };
    let slice: Vec<_> = all.into_iter().skip(start).take(limit + 1).collect();
    let next_cursor = if slice.len() > limit {
        slice.get(limit.saturating_sub(1)).map(|o| o.key.clone())
    } else {
        None
    };
    ListPage {
        objects: slice.into_iter().take(limit).collect(),
        next_cursor,
    }
}

/// Reads a body into memory, failing closed when it exceeds the scalar cap.
///
/// # Errors
///
/// Returns [`StorageError::PayloadTooLarge`] or I/O errors from `body`.
pub(crate) async fn read_capped_stream(
    mut body: Pin<Box<dyn AsyncRead + Send>>,
) -> bookclerk_storage::Result<Bytes> {
    let cap = MAX_SCALAR_BYTES as usize;
    let mut buf = Vec::new();
    let mut tmp = [0u8; 16 * 1024];
    loop {
        let n = body.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        if buf.len().saturating_add(n) > cap {
            return Err(StorageError::PayloadTooLarge(format!(
                "v1 adapter put_stream exceeded {MAX_SCALAR_BYTES} bytes"
            )));
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    Ok(Bytes::from(buf))
}

/// Wraps in-memory bytes as a streamed read (only after a size cap check).
#[must_use]
pub(crate) fn cursor_stream(data: Bytes) -> Pin<Box<dyn AsyncRead + Send>> {
    Box::pin(std::io::Cursor::new(data))
}

/// Builds a [`PutStreamResult`] after a fail-closed scalar put.
#[must_use]
pub(crate) fn put_result(bytes_written: u64) -> PutStreamResult {
    PutStreamResult {
        bytes_written,
        etag: None,
    }
}

/// Range is unsupported on the v1 JSON adapter.
///
/// # Errors
///
/// Returns [`StorageError::PayloadTooLarge`] when a range is requested, so the
/// host cannot pretend a partial scalar get is a streamed read.
pub(crate) fn reject_range(range: Option<ByteRange>) -> bookclerk_storage::Result<()> {
    if range.is_some() {
        return Err(StorageError::PayloadTooLarge(
            "v1 adapter does not support ranged get_stream (use api_version 2)".into(),
        ));
    }
    Ok(())
}

/// Fails closed when a probed object is larger than the scalar cap.
///
/// # Errors
///
/// Returns [`StorageError::PayloadTooLarge`] when `probe.size` is too large.
pub(crate) fn reject_oversize_probe(
    probe: &ObjectProbe,
    op: &str,
) -> bookclerk_storage::Result<()> {
    reject_oversize_scalar(probe.size, op)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bookclerk_plugin_sdk::v2::MAX_SCALAR_BYTES;
    use bytes::Bytes;

    #[test]
    fn reject_oversize_scalar_boundary() {
        reject_oversize_scalar(u64::from(MAX_SCALAR_BYTES), "put").unwrap();
        let err = reject_oversize_scalar(u64::from(MAX_SCALAR_BYTES) + 1, "put").unwrap_err();
        assert!(matches!(err, StorageError::PayloadTooLarge(_)));
    }

    #[test]
    fn paginate_objects_cursor() {
        let all = vec![
            ObjectInfo {
                key: "a".into(),
                size: 1,
            },
            ObjectInfo {
                key: "b".into(),
                size: 2,
            },
            ObjectInfo {
                key: "c".into(),
                size: 3,
            },
        ];
        let page = paginate_objects(all, Some("a"), 1);
        assert_eq!(page.objects.len(), 1);
        assert_eq!(page.objects[0].key, "b");
        assert_eq!(page.next_cursor.as_deref(), Some("b"));
    }

    #[tokio::test]
    async fn read_capped_stream_fail_closed() {
        let over = vec![0u8; MAX_SCALAR_BYTES as usize + 1];
        let err = read_capped_stream(Box::pin(std::io::Cursor::new(over)))
            .await
            .unwrap_err();
        assert!(matches!(err, StorageError::PayloadTooLarge(_)));
        let ok = read_capped_stream(Box::pin(std::io::Cursor::new(Bytes::from_static(b"ok"))))
            .await
            .unwrap();
        assert_eq!(ok.as_ref(), b"ok");
    }
}
