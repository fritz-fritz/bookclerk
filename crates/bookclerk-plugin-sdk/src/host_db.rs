//! Host-only database replay envelope types.
//!
//! Not part of the stable plugin-author API. Platform database guests and the
//! plugin host import these when implementing typed adapter sessions.

#[doc(hidden)]
pub use bookclerk_plugin_abi::{GuestReceiptPersist, HostExecuteEnvelope};
