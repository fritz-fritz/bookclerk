//! Audiobookshelf adapter modules.

mod client;
mod integration;

pub use client::{AbsApiClient, AbsLibrary, AbsUser};
pub use integration::AbsIntegration;
