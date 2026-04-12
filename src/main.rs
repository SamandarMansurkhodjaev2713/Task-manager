use telegram_task_bot::config::AppConfig;
use telegram_task_bot::domain::errors::AppResult;
use telegram_task_bot::infrastructure::logging::init_tracing;
use telegram_task_bot::presentation::bootstrap::run_application;

#[tokio::main]
async fn main() -> AppResult<()> {
    let config = AppConfig::from_env()?;
    init_tracing(&config.observability)?;
    run_application(config).await
}
