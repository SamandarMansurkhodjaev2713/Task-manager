//! Onboarding v2 — the FSM-driven first-run flow.
//!
//! The onboarding FSM is intentionally kept **thin**: it does not perform
//! task recovery, audit logging, or assignee intelligence itself.  Instead
//! it owns the *dialog* with the user (prompts / validation / retries) and
//!, on completion, delegates persistence of the resulting user record to
//! the existing [`RegisterUserUseCase::execute_with_link_decision`] path
//! from the `register_user` module.  This keeps two aspects separated:
//!
//! * **Presentation concern** — collecting first/last name and walking the
//!   user through the employee-selection screen.  Handled here.
//! * **Integration concern** — linking orphan tasks, audit rows, and the
//!   assignment-notification fan-out.  Kept in `register_user` so that
//!   pre-existing integration tests continue to exercise the same surface.
//!
//! The use case is explicitly written so that it:
//! * never panics on unexpected input (stray free-text while we wait for
//!   a button press => returns a `RetryPrompt` instead);
//! * uses optimistic concurrency via `UserRepository::save_onboarding_progress`
//!   so two parallel Telegram updates cannot leave the FSM in a torn state;
//! * returns a closed set of outcomes the dispatcher can pattern-match on
//!   to decide what to render next.

use std::sync::Arc;

use serde_json::json;

use crate::application::context::PrincipalContext;
use crate::application::dto::task_views::EmployeeCandidateView;
use crate::application::ports::repositories::{EmployeeRepository, UserRepository};
use crate::application::use_cases::register_user::{
    RegisterUserUseCase, RegistrationLinkClarification, RegistrationLinkDecision,
    RegistrationLinkPreview,
};
use crate::domain::errors::{AppError, AppResult};
use crate::domain::message::IncomingMessage;
use crate::domain::person_name::{PersonName, MAX_PERSON_NAME_PART_LENGTH};
use crate::domain::user::{OnboardingState, User};

/// The (free-form) text the user typed on the current onboarding step.
#[derive(Debug, Clone)]
pub struct OnboardingTextInput {
    pub text: String,
}

/// An operator-facing button / selection used to finish the employee-link
/// step without typing the name again.
#[derive(Debug, Clone, Copy)]
pub enum OnboardingLinkSelection {
    PickEmployee(i64),
    ContinueWithoutLink,
}

/// All legal outcomes the dispatcher has to handle after feeding an update
/// into `OnboardingUseCase`.
#[derive(Debug, Clone)]
pub enum OnboardingOutcome {
    /// Render the "type your first name" prompt.
    AskFirstName { user: User },
    /// Render the "type your last name" prompt.
    AskLastName { user: User },
    /// Render the employee-link screen (keyboard with candidates).
    AskEmployeeLink {
        user: User,
        clarification: OnboardingEmployeeLinkClarification,
    },
    /// Fully registered — dispatcher should show the welcome screen.
    Completed { user: User },
    /// User sent something we can't parse — dispatcher should reprompt.
    RetryPrompt { reason: OnboardingRetryReason },
}

#[derive(Debug, Clone)]
pub struct OnboardingEmployeeLinkClarification {
    pub message: String,
    pub candidates: Vec<EmployeeCandidateView>,
    pub allow_continue_unlinked: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum OnboardingRetryReason {
    FirstNameInvalid,
    LastNameInvalid,
    LinkSelectionExpected,
    TooLong,
}

/// The onboarding use case.  One instance per process — cheap to hold in an
/// `Arc` alongside the other use cases.
pub struct OnboardingUseCase {
    user_repository: Arc<dyn UserRepository>,
    employee_repository: Arc<dyn EmployeeRepository>,
    register_user_use_case: Arc<RegisterUserUseCase>,
}

impl OnboardingUseCase {
    pub fn new(
        user_repository: Arc<dyn UserRepository>,
        employee_repository: Arc<dyn EmployeeRepository>,
        register_user_use_case: Arc<RegisterUserUseCase>,
    ) -> Self {
        Self {
            user_repository,
            employee_repository,
            register_user_use_case,
        }
    }

    /// Handle the `/start` command (explicit or first-time).  Bootstraps a
    /// session row if missing and decides which prompt to render.
    pub async fn handle_start(
        &self,
        ctx: &PrincipalContext,
        message: &IncomingMessage,
    ) -> AppResult<OnboardingOutcome> {
        let user = self
            .user_repository
            .ensure_onboarding_session(
                message.sender_id,
                message.chat_id,
                message.sender_username.as_deref(),
                Some(message.sender_name.as_str()),
                ctx.now,
            )
            .await?;

        Ok(self.outcome_for(user))
    }

    /// Handle a free-text input while onboarding is in progress.  Unknown
    /// states short-circuit to `Completed` so legacy users never get stuck.
    pub async fn handle_text(
        &self,
        ctx: &PrincipalContext,
        message: &IncomingMessage,
        input: OnboardingTextInput,
    ) -> AppResult<OnboardingOutcome> {
        let user = self.require_active_session(message.sender_id).await?;

        match user.onboarding_state {
            OnboardingState::AwaitingFirstName => self.step_first_name(ctx, user, input).await,
            OnboardingState::AwaitingLastName => {
                self.step_last_name(ctx, user, input, message).await
            }
            OnboardingState::AwaitingEmployeeLink => Ok(OnboardingOutcome::RetryPrompt {
                reason: OnboardingRetryReason::LinkSelectionExpected,
            }),
            OnboardingState::Completed => Ok(OnboardingOutcome::Completed { user }),
        }
    }

    /// Handle an explicit button press on the employee-link step.
    pub async fn handle_link_selection(
        &self,
        ctx: &PrincipalContext,
        message: &IncomingMessage,
        selection: OnboardingLinkSelection,
    ) -> AppResult<OnboardingOutcome> {
        let user = self.require_active_session(message.sender_id).await?;

        let decision = match selection {
            OnboardingLinkSelection::PickEmployee(id) => RegistrationLinkDecision::EmployeeId(id),
            OnboardingLinkSelection::ContinueWithoutLink => {
                RegistrationLinkDecision::ContinueUnlinked
            }
        };

        let first = user
            .first_name
            .clone()
            .ok_or_else(|| invalid_state_error(&user))?;
        let last = user
            .last_name
            .clone()
            .ok_or_else(|| invalid_state_error(&user))?;
        let employee_id_to_link = match decision {
            RegistrationLinkDecision::EmployeeId(id) => Some(id),
            _ => None,
        };

        // Defence against torn persistence: if the session row somehow lacks
        // a database id we refuse with an internal error rather than panicking
        // (the previous `expect` would crash the dispatcher worker).
        let user_id = user.id.ok_or_else(|| invalid_state_error(&user))?;
        let completed_user = self
            .user_repository
            .complete_onboarding(
                user_id,
                user.onboarding_version,
                &first,
                &last,
                employee_id_to_link,
                ctx.now,
            )
            .await?;

        self.register_user_use_case
            .execute_with_link_decision(message, decision)
            .await?;

        Ok(OnboardingOutcome::Completed {
            user: completed_user,
        })
    }

    // ── Steps ──────────────────────────────────────────────────────────────

    async fn step_first_name(
        &self,
        ctx: &PrincipalContext,
        user: User,
        input: OnboardingTextInput,
    ) -> AppResult<OnboardingOutcome> {
        if input.text.chars().count() > MAX_PERSON_NAME_PART_LENGTH {
            return Ok(OnboardingOutcome::RetryPrompt {
                reason: OnboardingRetryReason::TooLong,
            });
        }

        let Ok(first) = PersonName::parse_first_only(&input.text) else {
            return Ok(OnboardingOutcome::RetryPrompt {
                reason: OnboardingRetryReason::FirstNameInvalid,
            });
        };

        let user_id = user.id.ok_or_else(|| invalid_state_error(&user))?;
        let updated = self
            .user_repository
            .save_onboarding_progress(
                user_id,
                user.onboarding_version,
                OnboardingState::AwaitingLastName,
                Some(&first),
                None,
                ctx.now,
            )
            .await?;

        Ok(OnboardingOutcome::AskLastName { user: updated })
    }

    async fn step_last_name(
        &self,
        ctx: &PrincipalContext,
        user: User,
        input: OnboardingTextInput,
        message: &IncomingMessage,
    ) -> AppResult<OnboardingOutcome> {
        if input.text.chars().count() > MAX_PERSON_NAME_PART_LENGTH {
            return Ok(OnboardingOutcome::RetryPrompt {
                reason: OnboardingRetryReason::TooLong,
            });
        }

        let first_raw = user.first_name.clone().unwrap_or_default();
        let Ok(name) = PersonName::parse_last_with_first(&first_raw, &input.text) else {
            return Ok(OnboardingOutcome::RetryPrompt {
                reason: OnboardingRetryReason::LastNameInvalid,
            });
        };

        // Persist the last name and advance to link-selection.  If the
        // directory match is unambiguous we immediately finalise onboarding.
        let user_id = user.id.ok_or_else(|| invalid_state_error(&user))?;
        let updated = self
            .user_repository
            .save_onboarding_progress(
                user_id,
                user.onboarding_version,
                OnboardingState::AwaitingEmployeeLink,
                None,
                Some(name.last()),
                ctx.now,
            )
            .await?;

        // Try to auto-resolve via existing preview_linking.  If it returns
        // `Ready(EmployeeId | ContinueUnlinked)` — finalise right away.
        match self.register_user_use_case.preview_linking(message).await? {
            RegistrationLinkPreview::Ready(decision) => {
                let employee_id_to_link = match decision {
                    RegistrationLinkDecision::EmployeeId(id) => Some(id),
                    _ => None,
                };
                let updated_id = updated.id.ok_or_else(|| invalid_state_error(&updated))?;
                let completed_user = self
                    .user_repository
                    .complete_onboarding(
                        updated_id,
                        updated.onboarding_version,
                        name.first(),
                        name.last(),
                        employee_id_to_link,
                        ctx.now,
                    )
                    .await?;
                self.register_user_use_case
                    .execute_with_link_decision(message, decision)
                    .await?;
                Ok(OnboardingOutcome::Completed {
                    user: completed_user,
                })
            }
            RegistrationLinkPreview::ClarificationRequired(clarification) => {
                Ok(OnboardingOutcome::AskEmployeeLink {
                    user: updated,
                    clarification: OnboardingEmployeeLinkClarification {
                        message: clarification.message,
                        candidates: clarification.candidates,
                        allow_continue_unlinked: clarification.allow_continue_unlinked,
                    },
                })
            }
        }
    }

    async fn require_active_session(&self, telegram_id: i64) -> AppResult<User> {
        let user = self
            .user_repository
            .find_by_telegram_id(telegram_id)
            .await?
            .ok_or_else(|| {
                AppError::not_found(
                    "USER_NOT_FOUND",
                    "Onboarding session does not exist; send /start first",
                    json!({ "telegram_id": telegram_id }),
                )
            })?;
        Ok(user)
    }

    /// Cheap read-only probe: "is this Telegram user currently in onboarding
    /// and, if so, at which step?"  Used by the dispatcher to decide whether
    /// a click on the employee-link keyboard should route to `OnboardingUseCase`
    /// or to the legacy `register_user` path.
    pub async fn probe_onboarding_state(&self, telegram_id: i64) -> AppResult<Option<User>> {
        self.user_repository.find_by_telegram_id(telegram_id).await
    }

    fn outcome_for(&self, user: User) -> OnboardingOutcome {
        match user.onboarding_state {
            OnboardingState::AwaitingFirstName => OnboardingOutcome::AskFirstName { user },
            OnboardingState::AwaitingLastName => OnboardingOutcome::AskLastName { user },
            OnboardingState::AwaitingEmployeeLink => OnboardingOutcome::AskEmployeeLink {
                user: user.clone(),
                clarification: OnboardingEmployeeLinkClarification {
                    message: "Выберите себя из списка сотрудников".to_owned(),
                    candidates: Vec::new(),
                    allow_continue_unlinked: true,
                },
            },
            OnboardingState::Completed => OnboardingOutcome::Completed { user },
        }
    }

    /// The dispatcher can call this when it needs to (re)display the
    /// employee-link clarification (e.g. after an inline button press on a
    /// stale screen).  It rebuilds the clarification from scratch using the
    /// existing registration pipeline so the list of candidates cannot
    /// drift from what `preview_linking` would yield.
    pub async fn fetch_link_clarification(
        &self,
        message: &IncomingMessage,
    ) -> AppResult<OnboardingEmployeeLinkClarification> {
        let employees = self.employee_repository.list_active().await?;
        let _ = employees; // explicit use to keep field live across refactors
        match self.register_user_use_case.preview_linking(message).await? {
            RegistrationLinkPreview::ClarificationRequired(RegistrationLinkClarification {
                message: text,
                candidates,
                allow_continue_unlinked,
            }) => Ok(OnboardingEmployeeLinkClarification {
                message: text,
                candidates,
                allow_continue_unlinked,
            }),
            RegistrationLinkPreview::Ready(_) => Ok(OnboardingEmployeeLinkClarification {
                message: "Мы нашли вас в справочнике сотрудников — подтвердите, чтобы закончить."
                    .to_owned(),
                candidates: Vec::new(),
                allow_continue_unlinked: true,
            }),
        }
    }
}

fn invalid_state_error(user: &User) -> AppError {
    AppError::internal(
        "ONBOARDING_STATE_INVALID",
        "Cannot complete onboarding without captured first/last name",
        json!({
            "user_id": user.id,
            "state": user.onboarding_state.as_storage_value(),
        }),
    )
}
