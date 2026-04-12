use axum::extract::State;
use axum::http::HeaderValue;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use metrics_exporter_prometheus::PrometheusHandle;
use serde_json::json;
use tokio::task::JoinHandle;
use tower_http::trace::TraceLayer;

use crate::config::HttpServerConfig;
use crate::domain::errors::{AppError, AppResult};

#[derive(Clone)]
struct HttpState {
    metrics_handle: PrometheusHandle,
}

pub fn spawn_http_server(
    config: HttpServerConfig,
    metrics_handle: PrometheusHandle,
) -> JoinHandle<AppResult<()>> {
    tokio::spawn(async move {
        let state = HttpState { metrics_handle };
        let app = Router::new()
            .route("/healthz", get(healthz))
            .route("/metrics", get(metrics))
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
    Json(json!({ "status": "ok" }))
}

async fn metrics(State(state): State<HttpState>) -> Response {
    let mut response = state.metrics_handle.render().into_response();
    response.headers_mut().insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; version=0.0.4"),
    );
    response
}
