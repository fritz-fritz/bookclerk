//! Minimal HTTP control plane for `libationd`.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use libation_config::Config;
use libation_library::{LiberateStatus, LibraryStore};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tower_http::trace::TraceLayer;

/// Shared daemon state.
pub struct AppState {
    pub config: RwLock<Config>,
    pub library: LibraryStore,
    pub jobs: RwLock<Vec<JobInfo>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobInfo {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub detail: Option<String>,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct StatusResponse {
    accounts: usize,
    books: usize,
    liberated: i64,
    pending: i64,
    listen: String,
    storage_backend: String,
}

#[derive(Debug, Serialize)]
struct ActionResponse {
    ok: bool,
    message: String,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/status", get(status))
        .route("/scan", post(trigger_scan))
        .route("/liberate", post(trigger_liberate))
        .route("/jobs", get(list_jobs))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}

async fn status(State(state): State<Arc<AppState>>) -> Result<Json<StatusResponse>, StatusCode> {
    let accounts = state
        .library
        .list_accounts()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .len();
    let books = state
        .library
        .list_books(None)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .len();
    let liberated = state
        .library
        .count_by_status(LiberateStatus::Liberated)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let pending = state
        .library
        .count_by_status(LiberateStatus::NotLiberated)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let cfg = state.config.read().await;
    Ok(Json(StatusResponse {
        accounts,
        books,
        liberated,
        pending,
        listen: cfg.daemon.listen.clone(),
        storage_backend: format!("{:?}", cfg.storage.backend).to_ascii_lowercase(),
    }))
}

async fn trigger_scan(State(state): State<Arc<AppState>>) -> Json<ActionResponse> {
    let id = format!("scan-{}", chrono_like_id());
    state.jobs.write().await.push(JobInfo {
        id: id.clone(),
        kind: "scan".into(),
        status: "accepted".into(),
        detail: Some("library sync wiring pending".into()),
    });
    tracing::info!(%id, "scan job accepted");
    Json(ActionResponse {
        ok: true,
        message: format!("scan job {id} accepted (sync pending)"),
    })
}

async fn trigger_liberate(State(state): State<Arc<AppState>>) -> Json<ActionResponse> {
    let id = format!("liberate-{}", chrono_like_id());
    state.jobs.write().await.push(JobInfo {
        id: id.clone(),
        kind: "liberate".into(),
        status: "accepted".into(),
        detail: Some("liberate pipeline wiring pending".into()),
    });
    tracing::info!(%id, "liberate job accepted");
    Json(ActionResponse {
        ok: true,
        message: format!("liberate job {id} accepted (pipeline pending)"),
    })
}

async fn list_jobs(State(state): State<Arc<AppState>>) -> Json<Vec<JobInfo>> {
    Json(state.jobs.read().await.clone())
}

fn chrono_like_id() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
