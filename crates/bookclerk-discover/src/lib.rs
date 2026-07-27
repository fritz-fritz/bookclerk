//! Discovery: works graph, embeddings, recommendations, purchase hints, requests.

mod embed;
mod error;
mod openlibrary;
mod purchase;
mod recommend;
mod works;

pub use embed::{
    default_embedding_model_id, embed_dirty_works, embedding_model_id, open_embedder,
    text_for_work, CosineHit, Embedder, HashEmbedder, MODEL_ALL_MINILM_L6_V2_Q,
    MODEL_LOCAL_HASH_V1,
};
pub use error::{DiscoverError, Result};
pub use openlibrary::enrich_books_from_openlibrary;
pub use purchase::{purchase_hints_for, PurchaseHint};
pub use recommend::{recommend, RecommendOptions, Recommendation};
pub use works::rebuild_works_from_library;

#[cfg(feature = "onnx-embeddings")]
pub use embed::OnnxEmbedder;
