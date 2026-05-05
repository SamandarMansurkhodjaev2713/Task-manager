use std::sync::Arc;
use std::time::Duration;

use chrono::{NaiveDate, Timelike, Utc};
use sqlx::SqlitePool;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::application::use_cases::enqueue_daily_summaries::EnqueueDailySummariesUseCase;
use crate::application::use_cases::enqueue_task_reminders::EnqueueTaskRemindersUseCase;
use crate::application::use_cases::process_notifications::ProcessNotificationsUseCase;
use crate::application::use_cases::process_recurrence_rules::ProcessRecurrenceRulesUseCase;
use crate::application::use_cases::sheets_write_back::SheetsWriteBackUseCase;
use crate::application::use_cases::sync_employees::SyncEmployeesUseCase;
use crate::application::use_cases::update_sla_states::UpdateSlaStatesUseCase;
use crate::config::SchedulerConfig;
use crate::infrastructure::db::backup::run_backup_cycle;

/// Interval between SLA escalation scans.  Five minutes is a reasonable
/// upper bound for how stale the `sla_state` column can be — short enough
/// to catch transitions before the next morning standup, long enough to
/// avoid hammering SQLite on idle systems.
const SLA_CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Interval between recurrence-rule scheduler ticks.  One minute means
/// CRON expressions can fire with ≤ 60-second jitter, which is acceptable
/// for task-management workflows.
const RECURRENCE_CHECK_INTERVAL: Duration = Duration::from_secs(60);

/// Interval between write-back flush runs.  Five minutes is a good balance:
/// short enough that newly registered employees appear in Sheets within one
/// work-hour, long enough to avoid hammering the Sheets API quota.
const WRITE_BACK_FLUSH_INTERVAL: Duration = Duration::from_secs(5 * 60);

pub struct BackgroundJobs {
    cancellation_token: CancellationToken,
    handles: Vec<JoinHandle<()>>,
}

pub struct BackgroundJobUseCases {
    pub sync_employees: Arc<SyncEmployeesUseCase>,
    pub process_notifications: Arc<ProcessNotificationsUseCase>,
    pub enqueue_task_reminders: Arc<EnqueueTaskRemindersUseCase>,
    pub enqueue_daily_summaries: Arc<EnqueueDailySummariesUseCase>,
    pub update_sla_states: Arc<UpdateSlaStatesUseCase>,
    pub process_recurrence_rules: Arc<ProcessRecurrenceRulesUseCase>,
    pub sheets_write_back: Arc<SheetsWriteBackUseCase>,
    /// SQLite pool — used only when `config.sqlite_backup_dir` is set.
    pub db_pool: SqlitePool,
}

impl BackgroundJobs {
    pub fn start(config: SchedulerConfig, use_cases: BackgroundJobUseCases) -> Self {
        let cancellation_token = CancellationToken::new();
        let deadline_reminders_use_case = use_cases.enqueue_task_reminders.clone();
        let overdue_reminders_use_case = use_cases.enqueue_task_reminders.clone();
        let daily_summaries_use_case = use_cases.enqueue_daily_summaries.clone();
        let sync_employees_use_case = use_cases.sync_employees;
        let process_notifications_use_case = use_cases.process_notifications;
        let update_sla_states_use_case = use_cases.update_sla_states;
        let process_recurrence_rules_use_case = use_cases.process_recurrence_rules;
        let sheets_write_back_use_case = use_cases.sheets_write_back;
        let db_pool = use_cases.db_pool;
        let mut handles = vec![
            spawn_interval_job(
                cancellation_token.clone(),
                Duration::from_secs(u64::from(config.employee_sync_interval_minutes.get()) * 60),
                move || {
                    let sync_employees_use_case = sync_employees_use_case.clone();
                    async move {
                        if let Err(error) = sync_employees_use_case.execute().await {
                            tracing::error!(code = error.code(), "employee_sync_failed");
                        }
                    }
                },
            ),
            spawn_interval_job(
                cancellation_token.clone(),
                Duration::from_secs(u64::from(config.notification_poll_interval_seconds.get())),
                move || {
                    let process_notifications_use_case = process_notifications_use_case.clone();
                    async move {
                        if let Err(error) = process_notifications_use_case.execute().await {
                            tracing::error!(code = error.code(), "notification_processing_failed");
                        }
                    }
                },
            ),
            spawn_daily_job(
                cancellation_token.clone(),
                Duration::from_secs(u64::from(config.reminder_tick_seconds.get())),
                config.daily_deadline_reminder_hour_utc,
                move || {
                    let enqueue_task_reminders_use_case = deadline_reminders_use_case.clone();
                    async move {
                        if let Err(error) = enqueue_task_reminders_use_case
                            .enqueue_upcoming_deadlines()
                            .await
                        {
                            tracing::error!(code = error.code(), "deadline_reminders_failed");
                        }
                    }
                },
            ),
            spawn_daily_job(
                cancellation_token.clone(),
                Duration::from_secs(u64::from(config.reminder_tick_seconds.get())),
                config.daily_overdue_scan_hour_utc,
                move || {
                    let enqueue_task_reminders_use_case = overdue_reminders_use_case.clone();
                    async move {
                        if let Err(error) = enqueue_task_reminders_use_case
                            .enqueue_overdue_alerts()
                            .await
                        {
                            tracing::error!(code = error.code(), "overdue_reminders_failed");
                        }
                    }
                },
            ),
            spawn_daily_job(
                cancellation_token.clone(),
                Duration::from_secs(u64::from(config.reminder_tick_seconds.get())),
                config.daily_summary_hour_utc,
                move || {
                    let enqueue_daily_summaries_use_case = daily_summaries_use_case.clone();
                    async move {
                        if let Err(error) = enqueue_daily_summaries_use_case.execute().await {
                            tracing::error!(code = error.code(), "daily_summary_enqueue_failed");
                        }
                    }
                },
            ),
            spawn_interval_job(cancellation_token.clone(), SLA_CHECK_INTERVAL, move || {
                let uc = update_sla_states_use_case.clone();
                async move {
                    if let Err(error) = uc.execute().await {
                        tracing::error!(code = error.code(), "sla_escalation_scan_failed");
                    }
                }
            }),
            spawn_interval_job(
                cancellation_token.clone(),
                RECURRENCE_CHECK_INTERVAL,
                move || {
                    let uc = process_recurrence_rules_use_case.clone();
                    async move {
                        if let Err(error) = uc.execute().await {
                            tracing::error!(
                                code = error.code(),
                                "recurrence_rules_processing_failed"
                            );
                        }
                    }
                },
            ),
            spawn_interval_job(
                cancellation_token.clone(),
                WRITE_BACK_FLUSH_INTERVAL,
                move || {
                    let uc = sheets_write_back_use_case.clone();
                    async move {
                        if let Err(error) = uc.flush().await {
                            tracing::error!(code = error.code(), "sheets_write_back_flush_failed");
                        }
                    }
                },
            ),
        ];

        // SQLite hot-backup job — only spawned when the operator has configured
        // a backup directory.  Uses VACUUM INTO which produces an atomically
        // consistent, WAL-free copy while normal reads/writes continue.
        if let Some(backup_dir) = config.sqlite_backup_dir.clone() {
            let max_files = config.sqlite_backup_max_files.get();
            let interval =
                Duration::from_secs(u64::from(config.sqlite_backup_interval_hours.get()) * 3600);
            let pool = db_pool.clone();
            tracing::info!(
                backup_dir = backup_dir,
                interval_hours = config.sqlite_backup_interval_hours.get(),
                max_files,
                "sqlite_backup: scheduled hot-backup enabled"
            );
            handles.push(spawn_interval_job(
                cancellation_token.clone(),
                interval,
                move || {
                    let pool = pool.clone();
                    let dir = backup_dir.clone();
                    async move {
                        run_backup_cycle(&pool, &dir, max_files).await;
                    }
                },
            ));
        }

        Self {
            cancellation_token,
            handles,
        }
    }

    pub async fn shutdown(self) {
        self.cancellation_token.cancel();
        for handle in self.handles {
            let _ = handle.await;
        }
    }
}

fn spawn_interval_job<F, Fut>(
    cancellation_token: CancellationToken,
    interval: Duration,
    job_factory: F,
) -> JoinHandle<()>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        loop {
            tokio::select! {
                _ = cancellation_token.cancelled() => break,
                _ = ticker.tick() => job_factory().await,
            }
        }
    })
}

fn spawn_daily_job<F, Fut>(
    cancellation_token: CancellationToken,
    tick_interval: Duration,
    target_hour_utc: u32,
    job_factory: F,
) -> JoinHandle<()>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(tick_interval);
        let mut last_run_date: Option<NaiveDate> = None;

        loop {
            tokio::select! {
                _ = cancellation_token.cancelled() => break,
                _ = ticker.tick() => {
                    let now = Utc::now();
                    let today = now.date_naive();
                    if now.hour() == target_hour_utc && last_run_date != Some(today) {
                        job_factory().await;
                        last_run_date = Some(today);
                    }
                }
            }
        }
    })
}
