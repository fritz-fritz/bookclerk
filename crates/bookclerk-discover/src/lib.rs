//! Discovery: works graph, embeddings, recommendations, purchase hints, requests.

mod candidates;
mod embed;
mod error;
mod openlibrary;
mod purchase;
mod recommend;
mod shelves;
mod works;

pub use candidates::{
    gather_storefront_candidates, select_taste_seeds, CandidateFetchOptions, StorefrontCandidate,
};
pub use embed::{
    default_embedding_model_id, embed_dirty_works, embedding_model_id, open_embedder,
    text_for_work, CosineHit, Embedder, HashEmbedder, MODEL_ALL_MINILM_L6_V2_Q,
    MODEL_LOCAL_HASH_V1,
};
pub use error::{DiscoverError, Result};
pub use openlibrary::{
    enrich_books_from_openlibrary, enrich_books_from_openlibrary_with, OpenLibraryOptions,
};
pub use purchase::{purchase_hints_for, PurchaseHint};
pub use recommend::{
    listening_engagement, parse_series_index, recommend, recommend_feed, RecommendOptions,
    Recommendation,
};
pub use shelves::{
    build_discover_feed, flatten_feed, shelf_is_disabled, shelf_kind_catalog, DiscoverFeed,
    DiscoverShelf, ShelfKindInfo, ShelfTaste,
};
pub use works::rebuild_works_from_library;

#[cfg(feature = "onnx-embeddings")]
pub use embed::OnnxEmbedder;
