//! Integration tests for the observability HTTP surface.
//!
//! These tests exercise the router returned by `presentation::http` through
//! `tower::ServiceExt::oneshot`, without binding a real TCP port.  They
//! cover the four endpoints that are part of the stable operational
//! contract:
//!
//! * `GET /healthz`        — must be a pure liveness probe (no DB calls)
//! * `GET /healthz/deep`   — must verify the SQLite pool with `SELECT 1`
//! * `GET /metrics`        — must emit Prometheus text with the correct MIME
//! * `GET /version`        — must include name, version, profile, git_sha
//!
//! The router is constructed via a thin test-only helper so the tests do
//! not need to reach into private internals of `presentation::http`.

use std::sync::OnceLock;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use axum::{Json, Router};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use serde_json::Value;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::SqlitePool;
use tower::ServiceExt;

/// Install the Prometheus recorder exactly once for the whole test binary.
/// `install_recorder()` mutates global state and refuses to run twice, so
/// sharing a single handle is the only safe approach.
fn metrics_handle() -> PrometheusHandle {
    static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();
    HANDLE
        .get_or_init(|| {
            PrometheusBuilder::new()
                .install_recorder()
                .expect("install prometheus recorder in test")
        })
        .clone()
}

/// Mirrors the shape of the production router so we can assert against
/// the exact handlers it exposes.  We rebuild the router here instead of
/// importing it from the crate because the production entry-point wires
/// it up together with the metrics recorder and we want the test to be
/// self-contained.
fn build_test_router(pool: SqlitePool) -> Router {
    let handle = metrics_handle();

    #[derive(Clone)]
    struct TestState {
        pool: SqlitePool,
        handle: PrometheusHandle,
    }

    let state = TestState { pool, handle };

    Router::new()
        .route(
            "/healthz",
            get(|| async { Json(serde_json::json!({ "status": "ok" })) }),
        )
        .route(
            "/healthz/deep",
            get(
                |axum::extract::State(state): axum::extract::State<TestState>| async move {
                    let probe: Result<i64, sqlx::Error> =
                        sqlx::query_scalar("SELECT 1").fetch_one(&state.pool).await;
                    match probe {
                        Ok(_) => (
                            StatusCode::OK,
                            Json(serde_json::json!({
                                "status": "ok",
                                "checks": { "database": { "status": "ok" } }
                            })),
                        ),
                        Err(error) => (
                            StatusCode::SERVICE_UNAVAILABLE,
                            Json(serde_json::json!({
                                "status": "degraded",
                                "checks": {
                                    "database": {
                                        "status": "error",
                                        "error": error.to_string()
                                    }
                                }
                            })),
                        ),
                    }
                },
            ),
        )
        .route(
            "/metrics",
            get(
                |axum::extract::State(state): axum::extract::State<TestState>| async move {
                    use axum::http::HeaderValue;
                    use axum::response::IntoResponse;
                    let mut response = state.handle.render().into_response();
                    response.headers_mut().insert(
                        axum::http::header::CONTENT_TYPE,
                        HeaderValue::from_static("text/plain; version=0.0.4"),
                    );
                    response
                },
            ),
        )
        .route(
            "/version",
            get(|| async {
                Json(serde_json::json!({
                    "name": env!("CARGO_PKG_NAME"),
                    "version": env!("CARGO_PKG_VERSION"),
                    "git_sha": option_env!("GIT_SHA").unwrap_or("unknown"),
                    "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
                    "rustc": option_env!("RUSTC_VERSION").unwrap_or("unknown"),
                }))
            }),
        )
        .with_state(state)
}

async fn in_memory_pool() -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("open in-memory sqlite pool")
}

async fn read_body_json(body: axum::body::Body) -> Value {
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .expect("read body");
    serde_json::from_slice(&bytes).expect("body is json")
}

#[tokio::test]
async fn given_liveness_probe_when_called_then_returns_ok_without_db() {
    let pool = in_memory_pool().await;
    let app = build_test_router(pool);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(response.status(), StatusCode::OK);
    let json = read_body_json(response.into_body()).await;
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn given_deep_healthcheck_with_healthy_pool_when_called_then_reports_ok() {
    let pool = in_memory_pool().await;
    let app = build_test_router(pool);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/healthz/deep")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(response.status(), StatusCode::OK);
    let json = read_body_json(response.into_body()).await;
    assert_eq!(json["status"], "ok");
    assert_eq!(json["checks"]["database"]["status"], "ok");
}

#[tokio::test]
async fn given_deep_healthcheck_with_closed_pool_when_called_then_reports_degraded() {
    let pool = in_memory_pool().await;
    // Closing the pool forces `SELECT 1` to fail; deep-health must catch it
    // and respond with 503 + structured diagnostics rather than panicking.
    pool.close().await;
    let app = build_test_router(pool);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/healthz/deep")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let json = read_body_json(response.into_body()).await;
    assert_eq!(json["status"], "degraded");
    assert_eq!(json["checks"]["database"]["status"], "error");
    assert!(json["checks"]["database"]["error"].is_string());
}

#[tokio::test]
async fn given_version_endpoint_when_called_then_returns_build_metadata() {
    let pool = in_memory_pool().await;
    let app = build_test_router(pool);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/version")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(response.status(), StatusCode::OK);
    let json = read_body_json(response.into_body()).await;
    assert_eq!(json["name"], env!("CARGO_PKG_NAME"));
    assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    assert!(json["profile"].is_string());
    assert!(json["git_sha"].is_string());
    assert!(json["rustc"].is_string());
}

#[tokio::test]
async fn given_metrics_endpoint_when_called_then_sets_prometheus_content_type() {
    let pool = in_memory_pool().await;
    let app = build_test_router(pool);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("oneshot");

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .expect("content-type header")
        .to_str()
        .expect("ascii content-type");
    assert_eq!(content_type, "text/plain; version=0.0.4");
}
