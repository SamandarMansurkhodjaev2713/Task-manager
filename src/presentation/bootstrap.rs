use std::sync::Arc;

use secrecy::ExposeSecret;
use teloxide::Bot;

use crate::application::ports::services::TelegramNotifier;
use crate::application::use_cases::add_task_comment::AddTaskCommentUseCase;
use crate::application::use_cases::assignee_resolution::AssigneeResolver;
use crate::application::use_cases::collect_stats::CollectStatsUseCase;
use crate::application::use_cases::create_task_from_message::CreateTaskFromMessageUseCase;
use crate::application::use_cases::enqueue_daily_summaries::EnqueueDailySummariesUseCase;
use crate::application::use_cases::enqueue_task_reminders::EnqueueTaskRemindersUseCase;
use crate::application::use_cases::get_task_status::GetTaskStatusUseCase;
use crate::application::use_cases::list_tasks::ListTasksUseCase;
use crate::application::use_cases::process_notifications::ProcessNotificationsUseCase;
use crate::application::use_cases::reassign_task::ReassignTaskUseCase;
use crate::application::use_cases::register_user::RegisterUserUseCase;
use crate::application::use_cases::report_task_blocker::ReportTaskBlockerUseCase;
use crate::application::use_cases::sync_employees::SyncEmployeesUseCase;
use crate::application::use_cases::update_task_status::UpdateTaskStatusUseCase;
use crate::config::AppConfig;
use crate::domain::errors::AppResult;
use crate::infrastructure::ai::gemini_client::GeminiTaskGenerator;
use crate::infrastructure::ai::local_task_generator::LocalTaskGenerator;
use crate::infrastructure::ai::openai_transcription_client::OpenAiTranscriptionClient;
use crate::infrastructure::clock::system_clock::SystemClock;
use crate::infrastructure::db::pool::connect;
use crate::infrastructure::db::repositories::{
    SqliteAuditLogRepository, SqliteCommentRepository, SqliteEmployeeRepository,
    SqliteNotificationRepository, SqliteTaskRepository, SqliteUserRepository,
};
use crate::infrastructure::employee_directory::google_sheets_client::GoogleSheetsEmployeeDirectory;
use crate::infrastructure::employee_directory::local_directory::LocalEmployeeDirectory;
use crate::infrastructure::logging::init_metrics;
use crate::infrastructure::scheduler::BackgroundJobs;
use crate::infrastructure::telegram::bot_gateway::TeloxideNotifier;
use crate::presentation::http::spawn_http_server;
use crate::presentation::telegram::active_screens::ActiveScreenStore;
use crate::presentation::telegram::dispatcher::{run_telegram_dispatcher, TelegramRuntime};
use crate::presentation::telegram::drafts::CreationSessionStore;
use crate::presentation::telegram::interactions::TaskInteractionSessionStore;
use crate::presentation::telegram::rate_limiter::TelegramRateLimiter;

pub async fn run_application(config: AppConfig) -> AppResult<()> {
    let metrics_handle = init_metrics()?;
    let pool = connect(&config.database.database_url).await?;

    let user_repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let employee_repository = Arc::new(SqliteEmployeeRepository::new(pool.clone()));
    let task_repository = Arc::new(SqliteTaskRepository::new(pool.clone()));
    let notification_repository = Arc::new(SqliteNotificationRepository::new(pool.clone()));
    let audit_log_repository = Arc::new(SqliteAuditLogRepository::new(pool.clone()));
    let comment_repository = Arc::new(SqliteCommentRepository::new(pool.clone()));

    let clock = Arc::new(SystemClock);
    let assignee_resolver = Arc::new(AssigneeResolver::new(
        user_repository.clone(),
        employee_repository.clone(),
    ));
    let task_generator: Arc<dyn crate::application::ports::services::TaskGenerator> =
        if is_placeholder(config.gemini.api_key.expose_secret()) {
            Arc::new(LocalTaskGenerator)
        } else {
            Arc::new(GeminiTaskGenerator::new(config.gemini.clone())?)
        };
    let speech_to_text_service = Arc::new(OpenAiTranscriptionClient::new(
        config.telegram.clone(),
        config.openai.clone(),
    )?);
    let employee_directory_gateway: Arc<
        dyn crate::application::ports::services::EmployeeDirectoryGateway,
    > = if is_placeholder(&config.google_sheets.spreadsheet_id)
        || (config.google_sheets.api_key.is_none() && config.google_sheets.bearer_token.is_none())
    {
        Arc::new(LocalEmployeeDirectory)
    } else {
        Arc::new(GoogleSheetsEmployeeDirectory::new(
            config.google_sheets.clone(),
        )?)
    };

    let bot = Bot::new(config.telegram.bot_token.expose_secret());
    let notifier = TeloxideNotifier::new(bot);
    let telegram_notifier: Arc<dyn TelegramNotifier> = Arc::new(notifier.clone());

    let create_task_use_case = Arc::new(CreateTaskFromMessageUseCase::new(
        clock.clone(),
        user_repository.clone(),
        task_repository.clone(),
        notification_repository.clone(),
        audit_log_repository.clone(),
        task_generator,
        speech_to_text_service,
        assignee_resolver.clone(),
    ));
    let list_tasks_use_case = Arc::new(ListTasksUseCase::new(
        clock.clone(),
        task_repository.clone(),
    ));
    let get_task_status_use_case = Arc::new(GetTaskStatusUseCase::new(
        task_repository.clone(),
        audit_log_repository.clone(),
        notification_repository.clone(),
        user_repository.clone(),
        employee_repository.clone(),
        comment_repository.clone(),
    ));
    let update_task_status_use_case = Arc::new(UpdateTaskStatusUseCase::new(
        clock.clone(),
        task_repository.clone(),
        notification_repository.clone(),
        audit_log_repository.clone(),
    ));
    let add_task_comment_use_case = Arc::new(AddTaskCommentUseCase::new(
        clock.clone(),
        task_repository.clone(),
        comment_repository.clone(),
        notification_repository.clone(),
        audit_log_repository.clone(),
    ));
    let report_task_blocker_use_case = Arc::new(ReportTaskBlockerUseCase::new(
        clock.clone(),
        task_repository.clone(),
        comment_repository.clone(),
        notification_repository.clone(),
        audit_log_repository.clone(),
    ));
    let reassign_task_use_case = Arc::new(ReassignTaskUseCase::new(
        clock.clone(),
        task_repository.clone(),
        notification_repository.clone(),
        audit_log_repository.clone(),
        assignee_resolver,
    ));
    let collect_stats_use_case = Arc::new(CollectStatsUseCase::new(task_repository.clone()));
    let register_user_use_case = Arc::new(RegisterUserUseCase::new(
        clock.clone(),
        user_repository.clone(),
        employee_repository.clone(),
        task_repository.clone(),
        notification_repository.clone(),
        audit_log_repository.clone(),
    ));
    let sync_employees_use_case = Arc::new(SyncEmployeesUseCase::new(
        employee_repository,
        employee_directory_gateway,
    ));
    let process_notifications_use_case = Arc::new(ProcessNotificationsUseCase::new(
        notification_repository.clone(),
        user_repository.clone(),
        task_repository.clone(),
        audit_log_repository.clone(),
        telegram_notifier,
    ));
    let enqueue_task_reminders_use_case = Arc::new(EnqueueTaskRemindersUseCase::new(
        clock.clone(),
        task_repository.clone(),
        notification_repository.clone(),
    ));
    let enqueue_daily_summaries_use_case = Arc::new(EnqueueDailySummariesUseCase::new(
        clock,
        user_repository,
        task_repository,
        notification_repository,
    ));

    let background_jobs = BackgroundJobs::start(
        config.scheduler.clone(),
        sync_employees_use_case.clone(),
        process_notifications_use_case.clone(),
        enqueue_task_reminders_use_case,
        enqueue_daily_summaries_use_case,
    );
    let http_server = spawn_http_server(config.http_server.clone(), metrics_handle);
    let telegram_runtime = TelegramRuntime {
        notifier,
        rate_limiter: TelegramRateLimiter::new(),
        active_screens: ActiveScreenStore::default(),
        creation_sessions: CreationSessionStore::default(),
        task_interactions: TaskInteractionSessionStore::default(),
        register_user_use_case,
        create_task_use_case,
        list_tasks_use_case,
        get_task_status_use_case,
        update_task_status_use_case,
        add_task_comment_use_case,
        report_task_blocker_use_case,
        reassign_task_use_case,
        collect_stats_use_case,
        sync_employees_use_case,
    };
    let telegram_handle = tokio::spawn(run_telegram_dispatcher(telegram_runtime));

    let shutdown_result = tokio::select! {
        result = http_server => result.unwrap_or_else(|join_error| Err(crate::domain::errors::AppError::internal(
            "HTTP_TASK_JOIN_FAILED",
            "HTTP server task crashed",
            serde_json::json!({ "error": join_error.to_string() }),
        ))),
        result = telegram_handle => result.unwrap_or_else(|join_error| Err(crate::domain::errors::AppError::internal(
            "TELEGRAM_TASK_JOIN_FAILED",
            "Telegram dispatcher crashed",
            serde_json::json!({ "error": join_error.to_string() }),
        ))),
        _ = shutdown_signal() => Ok(()),
    };

    background_jobs.shutdown().await;
    shutdown_result
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");

        tokio::select! {
            _ = ctrl_c => {},
            _ = sigterm.recv() => {},
        }
    }

    #[cfg(not(unix))]
    ctrl_c.await;
}

fn is_placeholder(value: &str) -> bool {
    value.trim().is_empty() || value == "replace_me" || value == "local_placeholder"
}
