//! Presentation-layer glue for the onboarding v2 flow.
//!
//! The dispatcher calls `render_outcome` after every `OnboardingUseCase`
//! turn.  This module owns:
//!
//! * the mapping from `OnboardingOutcome` → on-screen text + keyboard;
//! * the bookkeeping of the active screen so that stale callbacks can be
//!   detected;
//! * the registration-link registry (shared with the legacy registration
//!   flow) so that employee-pick buttons continue to work even after a
//!   Telegram client reconnect.

use teloxide::prelude::Requester;
use teloxide::types::ChatId;
use teloxide::Bot;

use crate::application::use_cases::onboarding::{
    OnboardingEmployeeLinkClarification, OnboardingOutcome, OnboardingRetryReason,
};
use crate::domain::user::User;
use crate::presentation::telegram::active_screens::ScreenDescriptor;
use crate::presentation::telegram::ui;

use super::dispatcher_transport::send_fresh_screen;
use super::TelegramRuntime;

/// Render the outcome produced by the onboarding use case.  `actor` is the
/// `User` row that just came back from the use case — it is `None` during
/// retry prompts because the user-row did not move.
pub(crate) async fn render_outcome(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    outcome: OnboardingOutcome,
) -> Result<OnboardingNextAction, teloxide::RequestError> {
    match outcome {
        OnboardingOutcome::AskFirstName { .. } => {
            send_fresh_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::OnboardingFirstName,
                &ui::onboarding_welcome_text(),
                teloxide::types::InlineKeyboardMarkup::default(),
            )
            .await?;
            Ok(OnboardingNextAction::AwaitText)
        }
        OnboardingOutcome::AskLastName { user } => {
            let first = user.first_name.clone().unwrap_or_default();
            send_fresh_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::OnboardingLastName,
                &ui::onboarding_ask_last_name_text(&first),
                teloxide::types::InlineKeyboardMarkup::default(),
            )
            .await?;
            Ok(OnboardingNextAction::AwaitText)
        }
        OnboardingOutcome::AskEmployeeLink {
            user: _,
            clarification,
        } => {
            publish_link_screen(bot, state, chat_id, clarification).await?;
            Ok(OnboardingNextAction::AwaitCallback)
        }
        OnboardingOutcome::Completed { user } => {
            announce_completion(bot, state, chat_id, &user).await?;
            Ok(OnboardingNextAction::Completed)
        }
        OnboardingOutcome::RetryPrompt { reason } => {
            let text = match reason {
                OnboardingRetryReason::FirstNameInvalid => ui::onboarding_retry_first_name_text(),
                OnboardingRetryReason::LastNameInvalid => ui::onboarding_retry_last_name_text(),
                OnboardingRetryReason::LinkSelectionExpected => ui::onboarding_link_expected_text(),
                OnboardingRetryReason::TooLong => ui::onboarding_too_long_text(),
            };
            bot.send_message(chat_id, text).await?;
            Ok(OnboardingNextAction::AwaitText)
        }
    }
}

async fn publish_link_screen(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    clarification: OnboardingEmployeeLinkClarification,
) -> Result<(), teloxide::RequestError> {
    state
        .registration_links
        .set(
            chat_id.0,
            clarification
                .candidates
                .iter()
                .filter_map(|candidate| candidate.employee_id)
                .collect(),
            clarification.allow_continue_unlinked,
        )
        .await;
    send_fresh_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::RegistrationLinking,
        &ui::registration_link_text(&clarification.message, &clarification.candidates),
        ui::registration_link_keyboard(
            &clarification.candidates,
            clarification.allow_continue_unlinked,
        ),
    )
    .await?;
    Ok(())
}

async fn announce_completion(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    actor: &User,
) -> Result<(), teloxide::RequestError> {
    // Must use the same transport as other onboarding screens so the per-update
    // UX barrier is consumed.  A raw `send_message` left the barrier unspent so
    // a follow-up `send_error` on the same update could still fire — the
    // classic "profile created" + "Некорректный запрос" double reply.
    send_fresh_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::MainMenu,
        &ui::onboarding_completed_text(actor),
        teloxide::types::InlineKeyboardMarkup::default(),
    )
    .await
}

/// Tells the caller whether onboarding finished or is still awaiting input,
/// so the dispatcher can decide whether to immediately show the main menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OnboardingNextAction {
    AwaitText,
    AwaitCallback,
    Completed,
}
