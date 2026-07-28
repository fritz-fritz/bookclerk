//! Block on a database future from sync `LibraryStore` methods.

use std::future::Future;

/// Run an async DB operation from sync code.
///
/// Prefer calling from an existing Tokio runtime (`bookclerk` / `bookclerkd`).
/// Tests without a runtime get a current-thread runtime.
pub fn block_on_db<T>(fut: impl Future<Output = T>) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(fut)),
        Err(_) => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build tokio runtime for library database");
            rt.block_on(fut)
        }
    }
}
