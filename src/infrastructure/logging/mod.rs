use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use tracing_subscriber::EnvFilter;

use crate::config::ObservabilityConfig;
use crate::domain::errors::{AppError, AppResult};

pub fn init_tracing(config: &ObservabilityConfig) -> AppResult<()> {
    let filter = EnvFilter::try_new(&config.log_level).map_err(|error| {
        AppError::schema_validation(
            "RUST_LOG_INVALID",
            "RUST_LOG contains an invalid filter",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .json()
        .with_current_span(true)
        .with_span_list(true)
        .init();
    Ok(())
}

pub fn init_metrics() -> AppResult<PrometheusHandle> {
    PrometheusBuilder::new()
        .install_recorder()
        .map_err(|error| {
            AppError::internal(
                "PROMETHEUS_RECORDER_INIT_FAILED",
                "Failed to initialize Prometheus recorder",
                serde_json::json!({ "error": error.to_string() }),
            )
        })
}
