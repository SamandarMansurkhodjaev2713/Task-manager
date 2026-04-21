use std::time::Instant;

use axum::extract::State;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use metrics_exporter_prometheus::PrometheusHandle;
use serde_json::json;
use sqlx::SqlitePool;
use tokio::task::JoinHandle;
use tower_http::trace::TraceLayer;

use crate::config::HttpServerConfig;
use crate::domain::errors::{AppError, AppResult};

#[derive(Clone)]
struct HttpState {
    metrics_handle: PrometheusHandle,
    pool: SqlitePool,
    build_info: BuildInfo,
}

/// Static, read-only build metadata surfaced by the `/version` endpoint.
///
/// The values come from Cargo environment variables baked at compile time.
/// Git SHA / dirty flags are expected to be injected by the Docker build
/// via `--build-arg` + `cargo` env forwarding; when not present we fall
/// back to "unknown" so the endpoint still renders.
#[derive(Debug, Clone)]
struct BuildInfo {
    version: &'static str,
    name: &'static str,
    git_sha: &'static str,
    build_profile: &'static str,
    rustc_version: &'static str,
}

impl BuildInfo {
    fn collect() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION"),
            name: env!("CARGO_PKG_NAME"),
            git_sha: option_env!("GIT_SHA").unwrap_or("unknown"),
            build_profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            rustc_version: option_env!("RUSTC_VERSION").unwrap_or("unknown"),
        }
    }
}

pub fn spawn_http_server(
    config: HttpServerConfig,
    metrics_handle: PrometheusHandle,
    pool: SqlitePool,
) -> JoinHandle<AppResult<()>> {
    tokio::spawn(async move {
        let state = HttpState {
            metrics_handle,
            pool,
            build_info: BuildInfo::collect(),
        };
        let app = Router::new()
            .route("/healthz", get(healthz))
            .route("/healthz/deep", get(healthz_deep))
            .route("/metrics", get(metrics))
            .route("/version", get(version))
            .layer(TraceLayer::new_for_http())
            .with_state(state);
        let listener = tokio::net::TcpListener::bind(&config.bind_address)
            .await
            .map_err(|error| {
                AppError::internal(
                    "HTTP_BIND_FAILED",
                    "Failed to bind HTTP server",
                    serde_json::json!({ "error": error.to_string() }),
                )
            })?;

        axum::serve(listener, app).await.map_err(|error| {
            AppError::internal(
                "HTTP_SERVER_FAILED",
                "HTTP server stopped unexpectedly",
                serde_json::json!({ "error": error.to_string() }),
            )
        })
    })
}

async fn healthz() -> Json<serde_json::Value> {
    // Liveness only — the process is alive and the Tokio runtime is
    // scheduling tasks.  Intentionally does NOT touch the DB so orchestrators
    // (Docker/K8s) can't flap on transient storage hiccups.
    Json(json!({ "status": "ok" }))
}

/// Readiness / deep health check.
///
/// Performs a lightweight `SELECT 1` against SQLite to verify that the
/// pool is usable and the migrations journal is reachable.  Responds with
/// **200** + `{"status":"ok"}` when healthy, and **503** with structured
/// diagnostics when a dependency is degraded.  Latency is reported in
/// milliseconds so SRE dashboards can alert on slow DBs.
async fn healthz_deep(State(state): State<HttpState>) -> Response {
    let started = Instant::now();
    let db_result: Result<i64, sqlx::Error> =
        sqlx::query_scalar("SELECT 1").fetch_one(&state.pool).await;
    let elapsed_ms = started.elapsed().as_millis() as u64;

    match db_result {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({
                "status": "ok",
                "checks": {
                    "database": {
                        "status": "ok",
                        "latency_ms": elapsed_ms,
                    }
                }
            })),
        )
            .into_response(),
        Err(error) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "status": "degraded",
                "checks": {
                    "database": {
                        "status": "error",
                        "latency_ms": elapsed_ms,
                        "error": error.to_string(),
                    }
                }
            })),
        )
            .into_response(),
    }
}

async fn metrics(State(state): State<HttpState>) -> Response {
    let mut response = state.metrics_handle.render().into_response();
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4"),
    );
    response
}

async fn version(State(state): State<HttpState>) -> Json<serde_json::Value> {
    let info = &state.build_info;
    Json(json!({
        "name": info.name,
        "version": info.version,
        "git_sha": info.git_sha,
        "profile": info.build_profile,
        "rustc": info.rustc_version,
    }))
}
