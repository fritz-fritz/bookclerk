//! Audiobookshelf adapter modules.

mod brand;
mod client;
pub mod guest;
mod integration;
mod listening;

pub use brand::{brand_for_id, matches_id, BRAND};
pub use client::{
    AbsApiClient, AbsLibrary, AbsLibraryItem, AbsMediaProgress, AbsUser, AbsUserDetail,
};
pub use guest::AbsGuestState;
pub use integration::AbsIntegration;
pub use listening::{collect_listening_snapshots, sync_listening_progress};
