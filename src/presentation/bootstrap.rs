use std::sync::Arc;

use secrecy::ExposeSecret;
use teloxide::Bot;

use crate::application::ports::repositories::{EmployeeRepository, FeatureFlagRepository};
use crate::application::ports::services::TelegramNotifier;
use crate::application::use_cases::add_task_comment::AddTaskCommentUseCase;
use crate::application::use_cases::admin::AdminUseCase;
use crate::application::use_cases::assignee_resolution::AssigneeResolver;
use crate::application::use_cases::bootstrap_admins::BootstrapAdminsUseCase;
use crate::application::use_cases::collect_stats::CollectStatsUseCase;
use crate::application::use_cases::create_task_from_message::CreateTaskFromMessageUseCase;
use crate::application::use_cases::enqueue_daily_summaries::EnqueueDailySummariesUseCase;
use crate::application::use_cases::enqueue_task_reminders::EnqueueTaskRemindersUseCase;
use crate::application::use_cases::get_task_status::GetTaskStatusUseCase;
use crate::application::use_cases::list_tasks::ListTasksUseCase;
use crate::application::use_cases::onboarding::OnboardingUseCase;
use crate::application::use_cases::process_notifications::ProcessNotificationsUseCase;
use crate::application::use_cases::process_recurrence_rules::ProcessRecurrenceRulesUseCase;
use crate::application::use_cases::reassign_task::ReassignTaskUseCase;
use crate::application::use_cases::register_user::RegisterUserUseCase;
use crate::application::use_cases::report_task_blocker::ReportTaskBlockerUseCase;
use crate::application::use_cases::search_tasks::SearchTasksUseCase;
use crate::application::use_cases::seed_aliases::SeedAliasesUseCase;
use crate::application::use_cases::sheets_write_back::SheetsWriteBackUseCase;
use crate::application::use_cases::sync_employees::SyncEmployeesUseCase;
use crate::application::use_cases::update_sla_states::UpdateSlaStatesUseCase;
use crate::application::use_cases::update_task_status::UpdateTaskStatusUseCase;
use crate::config::AppConfig;
use crate::domain::errors::AppResult;
use crate::infrastructure::ai::directory_digest::EmployeeDirectoryDigest;
use crate::infrastructure::ai::gemini_client::GeminiTaskGenerator;
use crate::infrastructure::ai::local_task_generator::LocalTaskGenerator;
use crate::infrastructure::ai::openai_transcription_client::OpenAiTranscriptionClient;
use crate::infrastructure::clock::system_clock::SystemClock;
use crate::infrastructure::db::pool::connect;
use crate::infrastructure::db::repositories::{
    SqliteAdminAuditLogRepository, SqliteAliasRepository, SqliteAssigneeHistoryRepository,
    SqliteAuditLogRepository, SqliteCommentRepository, SqliteEmployeeRepository,
    SqliteFeatureFlagRepository, SqliteNotificationRepository, SqliteRecurrenceRepository,
    SqliteSecurityAuditLogRepository, SqliteSheetsSyncRepository, SqliteSlaRepository,
    SqliteTaskRepository, SqliteUserRepository,
};
use crate::infrastructure::employee_directory::google_sheets_client::GoogleSheetsEmployeeDirectory;
use crate::infrastructure::employee_directory::local_directory::LocalEmployeeDirectory;
use crate::infrastructure::employee_directory::sheets_write_back_client::GoogleSheetsWriteBackClient;
use crate::infrastructure::logging::init_metrics;
use crate::infrastructure::scheduler::{BackgroundJobUseCases, BackgroundJobs};
use crate::infrastructure::telegram::bot_gateway::TeloxideNotifier;
use crate::presentation::http::spawn_http_server;
use crate::presentation::telegram::active_screens::ActiveScreenStore;
use crate::presentation::telegram::admin_nonce_store::AdminNonceStore;
use crate::presentation::telegram::assignee_selections::PendingAssigneeSelectionStore;
use crate::presentation::telegram::dispatcher::{run_telegram_dispatcher, TelegramRuntime};
use crate::presentation::telegram::drafts::CreationSessionStore;
use crate::presentation::telegram::gateway::{ChatSerializer, UpdateDedup};
use crate::presentation::telegram::interactions::TaskInteractionSessionStore;
use crate::presentation::telegram::rate_limiter::TelegramRateLimiter;
use crate::presentation::telegram::registration_links::PendingRegistrationLinkStore;
use crate::shared::feature_flags::FeatureFlagRegistry;

pub async fn run_application(config: AppConfig) -> AppResult<()> {
    let metrics_handle = init_metrics()?;
    let pool = connect(&config.database.database_url).await?;

    let user_repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let employee_repository = Arc::new(SqliteEmployeeRepository::new(pool.clone()));

    // ── One-shot employee reset (idempotency-guarded) ───────────────────
    // When the operator sets `RESET_EMPLOYEES_ON_STARTUP=true`, wipe the
    // employee directory before doing anything else.  This must run BEFORE
    // the initial Sheets/CSV sync below so we don't immediately re-import
    // the data we are trying to clear.
    //
    // **Idempotency guard**: we record the calendar date of each wipe in the
    // `idempotency_keys` table.  Subsequent restarts on the same day — e.g.
    // after a crash-loop — skip the wipe even when the flag is still `true`.
    // This prevents the "forgot to unset the flag → restart → full wipe" foot-
    // gun that has cost operators their employee directories.  The flag becomes
    // effective again on the next calendar day.
    if config.bot.reset_employees_on_startup {
        let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let idempotency_result = sqlx::query(
            "INSERT OR IGNORE INTO idempotency_keys (use_case, key, created_at) \
             VALUES ('system:reset_employees', ?, CURRENT_TIMESTAMP)",
        )
        .bind(&today)
        .execute(&pool)
        .await;

        match idempotency_result {
            Ok(result) if result.rows_affected() == 0 => {
                // Row already existed → we already wiped today. Skip.
                tracing::warn!(
                    date = today,
                    "RESET_EMPLOYEES_ON_STARTUP=true but wipe was already performed today; \
                     skipping to avoid data loss on restart. \
                     Set the flag back to false when the directory is correct."
                );
            }
            Ok(_) => {
                // New row inserted → first wipe of the day; proceed.
                match EmployeeRepository::reset_all(employee_repository.as_ref()).await {
                    Ok(deleted) => tracing::warn!(
                        deleted,
                        date = today,
                        "RESET_EMPLOYEES_ON_STARTUP=true: employee directory wiped clean. \
                         Set the flag back to false to avoid wiping again tomorrow."
                    ),
                    Err(error) => {
                        let code = error.code();
                        tracing::error!(
                            code,
                            "RESET_EMPLOYEES_ON_STARTUP=true but the wipe failed; \
                             continuing with the existing data"
                        );
                    }
                }
            }
            Err(db_err) => {
                tracing::error!(
                    error = %db_err,
                    "RESET_EMPLOYEES_ON_STARTUP=true but idempotency check failed; \
                     skipping wipe to be safe"
                );
            }
        }
    }
    let task_repository = Arc::new(SqliteTaskRepository::new(pool.clone()));
    let notification_repository = Arc::new(SqliteNotificationRepository::new(pool.clone()));
    let audit_log_repository = Arc::new(SqliteAuditLogRepository::new(pool.clone()));
    let comment_repository = Arc::new(SqliteCommentRepository::new(pool.clone()));
    let admin_audit_repository = Arc::new(SqliteAdminAuditLogRepository::new(pool.clone()));
    let feature_flag_repository = Arc::new(SqliteFeatureFlagRepository::new(pool.clone()));
    let security_audit_repository = Arc::new(SqliteSecurityAuditLogRepository::new(pool.clone()));
    let alias_repository = Arc::new(SqliteAliasRepository::new(pool.clone()));
    let assignee_history_repository = Arc::new(SqliteAssigneeHistoryRepository::new(pool.clone()));
    let sheets_sync_repository = Arc::new(SqliteSheetsSyncRepository::new(pool.clone()));

    // ── Feature flags ─────────────────────────────────────────────────────
    // Build the in-memory registry from ENV defaults, then layer in any
    // admin-overrides that were persisted in the DB from previous runs.
    let db_flag_overrides = feature_flag_repository.list_overrides().await?;
    let mut flag_registry =
        FeatureFlagRegistry::from_env_and_defaults(config.features.env_value.as_deref());
    flag_registry.apply_overrides(&db_flag_overrides);
    let shared_flags = std::sync::Arc::new(tokio::sync::RwLock::new(flag_registry));

    let clock = Arc::new(SystemClock);

    // ── RBAC bootstrap ─────────────────────────────────────────────────────
    // This MUST run before the Telegram dispatcher starts handling updates
    // so that privileged operations (e.g. `/admin`) succeed on first use
    // for operators listed in `TELEGRAM_ADMIN_IDS`.  Missing users (those
    // who have not sent `/start` yet) are silently skipped; they will be
    // promoted on their next `/start` via a fallback check inside the
    // onboarding / registration path (Phase 4).
    let bootstrap_admins_use_case = BootstrapAdminsUseCase::new(
        clock.clone(),
        user_repository.clone(),
        admin_audit_repository.clone(),
    );
    bootstrap_admins_use_case
        .execute(&config.security.admin_ids)
        .await?;

    let assignee_resolver = Arc::new(
        AssigneeResolver::new(user_repository.clone(), employee_repository.clone())
            .with_aliases(alias_repository.clone())
            .with_history(assignee_history_repository),
    );
    let directory_digest_provider: Arc<
        dyn crate::application::ports::services::DirectoryDigestProvider,
    > = Arc::new(EmployeeDirectoryDigest::new(employee_repository.clone()));
    let task_generator: Arc<dyn crate::application::ports::services::TaskGenerator> =
        if is_placeholder(config.gemini.api_key.expose_secret()) {
            Arc::new(LocalTaskGenerator)
        } else {
            Arc::new(
                GeminiTaskGenerator::new(config.gemini.clone())?
                    .with_directory_digest(directory_digest_provider.clone()),
            )
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
        match config.google_sheets.local_csv_path.clone() {
            Some(path) => {
                tracing::info!(path, "using local employee directory CSV fallback");
                Arc::new(LocalEmployeeDirectory::from_csv(path))
            }
            None => Arc::new(LocalEmployeeDirectory::empty()),
        }
    } else {
        Arc::new(GoogleSheetsEmployeeDirectory::new(
            config.google_sheets.clone(),
        )?)
    };

    // ── Sheets write-back ─────────────────────────────────────────────────
    // Only active when write_back_range AND bearer_token are both configured.
    let sheets_write_back = {
        let range = config.google_sheets.write_back_range.as_deref();
        let has_sheet = !is_placeholder(&config.google_sheets.spreadsheet_id);
        let has_bearer = config.google_sheets.bearer_token.is_some();
        if let (Some(range), true, true) = (range, has_sheet, has_bearer) {
            match GoogleSheetsWriteBackClient::new(config.google_sheets.clone(), range.to_owned()) {
                Ok(gateway) => {
                    tracing::info!(range, "sheets write-back enabled");
                    Arc::new(SheetsWriteBackUseCase::new(
                        sheets_sync_repository.clone(),
                        Arc::new(gateway),
                    ))
                }
                Err(error) => {
                    tracing::warn!(
                        code = error.code(),
                        "sheets write-back gateway failed to build; running disabled"
                    );
                    Arc::new(SheetsWriteBackUseCase::disabled(
                        sheets_sync_repository.clone(),
                    ))
                }
            }
        } else {
            tracing::info!("sheets write-back disabled (GOOGLE_SHEETS_WRITE_BACK_RANGE or GOOGLE_SHEETS_BEARER_TOKEN not set)");
            Arc::new(SheetsWriteBackUseCase::disabled(
                sheets_sync_repository.clone(),
            ))
        }
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
        assignee_resolver.clone(),
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
    let onboarding_use_case = Arc::new(OnboardingUseCase::new(
        user_repository.clone(),
        employee_repository.clone(),
        register_user_use_case.clone(),
    ));
    let admin_use_case = Arc::new(AdminUseCase::new(
        clock.clone(),
        user_repository.clone(),
        admin_audit_repository.clone(),
        feature_flag_repository,
        shared_flags.clone(),
    ));
    let search_tasks_use_case = Arc::new(SearchTasksUseCase::new(task_repository.clone()));
    let sync_employees_use_case = Arc::new(SyncEmployeesUseCase::new(
        employee_repository.clone(),
        employee_directory_gateway,
    ));

    // Load the employee directory once before Telegram starts accepting
    // updates. This makes Docker/demo deployments immediately usable and lets
    // alias seeding bind short names to real employee IDs on the same startup
    // rather than requiring a second restart.
    match sync_employees_use_case.execute().await {
        Ok(count) => tracing::info!(count, "initial employee sync completed"),
        Err(error) => tracing::warn!(
            code = error.code(),
            "initial employee sync failed; background scheduler will retry"
        ),
    }
    match register_user_use_case
        .reconcile_existing_directory_links()
        .await
    {
        Ok(count) if count > 0 => tracing::info!(
            count,
            "existing registered users linked after employee directory sync"
        ),
        Ok(_) => {}
        Err(error) => tracing::warn!(
            code = error.code(),
            "existing user link reconciliation failed after employee directory sync"
        ),
    }

    // Phase 2: seed Russian diminutive aliases (idempotent). Runs best-effort:
    // failure logs a warning but does not abort startup.
    let seed_aliases_use_case =
        SeedAliasesUseCase::new(employee_repository.clone(), alias_repository.clone());
    if let Err(error) = seed_aliases_use_case.execute().await {
        tracing::warn!(
            code = error.code(),
            "seed_aliases failed; aliases will not be available until next restart"
        );
    }

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
        clock.clone(),
        user_repository.clone(),
        task_repository.clone(),
        notification_repository.clone(),
    ));
    let sla_repository = Arc::new(SqliteSlaRepository::new(pool.clone()));
    let recurrence_repository = Arc::new(SqliteRecurrenceRepository::new(pool.clone()));
    let update_sla_states_use_case = Arc::new(UpdateSlaStatesUseCase::new(
        clock.clone(),
        sla_repository,
        notification_repository,
        shared_flags.clone(),
    ));
    let process_recurrence_rules_use_case = Arc::new(ProcessRecurrenceRulesUseCase::new(
        clock,
        recurrence_repository,
        task_repository,
        shared_flags.clone(),
    ));

    let background_jobs = BackgroundJobs::start(
        config.scheduler.clone(),
        BackgroundJobUseCases {
            sync_employees: sync_employees_use_case.clone(),
            register_user: register_user_use_case.clone(),
            process_notifications: process_notifications_use_case.clone(),
            enqueue_task_reminders: enqueue_task_reminders_use_case,
            enqueue_daily_summaries: enqueue_daily_summaries_use_case,
            update_sla_states: update_sla_states_use_case,
            process_recurrence_rules: process_recurrence_rules_use_case,
            sheets_write_back: sheets_write_back.clone(),
            db_pool: pool.clone(),
        },
    );
    let http_server = spawn_http_server(config.http_server.clone(), metrics_handle, pool.clone());
    let telegram_runtime = TelegramRuntime {
        notifier,
        rate_limiter: TelegramRateLimiter::new(config.bot.rate_limit_per_minute),
        active_screens: ActiveScreenStore::default(),
        assignee_selections: PendingAssigneeSelectionStore::default(),
        registration_links: PendingRegistrationLinkStore::default(),
        creation_sessions: CreationSessionStore::default(),
        task_interactions: TaskInteractionSessionStore::default(),
        admin_nonce_store: AdminNonceStore::new(config.security.admin_nonce_ttl_seconds),
        assignee_resolver,
        sheets_write_back,
        user_repository: user_repository.clone(),
        register_user_use_case,
        onboarding_use_case,
        create_task_use_case,
        list_tasks_use_case,
        get_task_status_use_case,
        update_task_status_use_case,
        add_task_comment_use_case,
        report_task_blocker_use_case,
        reassign_task_use_case,
        collect_stats_use_case,
        sync_employees_use_case,
        admin_use_case,
        search_tasks_use_case,
        feature_flags: shared_flags,
        security_audit: security_audit_repository,
        chat_serializer: ChatSerializer::new(),
        update_dedup: UpdateDedup::new(),
        current_barriers: std::sync::Arc::new(tokio::sync::Mutex::new(
            std::collections::HashMap::new(),
        )),
    };
    // Register the default bot command menu before starting the dispatcher.
    // This is best-effort — failure is logged but never propagated so the bot
    // starts even if Telegram's setMyCommands endpoint is temporarily slow.
    crate::presentation::telegram::bot_commands::register_default_commands(
        &telegram_runtime.notifier.bot(),
    )
    .await;

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
