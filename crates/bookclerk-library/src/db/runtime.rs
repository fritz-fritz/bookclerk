//! Block on a database future from sync `LibraryStore` methods.

use std::future::Future;

/// Run an async DB operation from sync code.
///
/// - Multi-thread Tokio (`bookclerk` / `bookclerkd`): `block_in_place` + `Handle::block_on`.
/// - Current-thread Tokio (default `#[tokio::test]`): a helper thread with its own
///   runtime — `block_in_place` is unavailable and nesting `block_on` panics.
/// - No runtime: build a short-lived current-thread runtime.
pub fn block_on_db<T: Send>(fut: impl Future<Output = T> + Send) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(handle)
            if matches!(
                handle.runtime_flavor(),
                tokio::runtime::RuntimeFlavor::MultiThread
            ) =>
        {
            tokio::task::block_in_place(|| handle.block_on(fut))
        }
        Ok(_) => std::thread::scope(|s| {
            s.spawn(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build helper tokio runtime for library database")
                    .block_on(fut)
            })
            .join()
            .expect("library database helper thread panicked")
        }),
        Err(_) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime for library database")
            .block_on(fut),
    }
}
