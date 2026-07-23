//! GraphicAudio content source: auth, library sync, and DRM-free download.
//!
//! Uses the reverse-engineered Android Retrofit API under
//! `https://www.graphicaudio.net/access/`.

mod auth;
mod client;
mod download;
mod error;
mod source;
mod sync;

pub use auth::{
    auth_file_for, auth_file_for_account, auth_stem, find_auth_file, list_auth_files, load_auth,
    save_auth, GraphicAudioAuthFile,
};
pub use client::{
    DownloadLinks, GraphicAudioClient, Product, DEFAULT_BASE_URL, LOGIN_PATH, PRODUCTS_PATH,
};
pub use download::fetch_title_materials;
pub use error::{GraphicAudioError, Result};
pub use source::GraphicAudioSource;
pub use sync::{product_to_new_book, scan_library, ScanOptions};
