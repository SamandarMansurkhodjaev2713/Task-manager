use teloxide::types::{ChatId, InlineKeyboardMarkup};
use teloxide::Bot;
use uuid::Uuid;

use crate::application::context::{PrincipalContext, TelegramChatContext};
use crate::application::use_cases::onboarding::OnboardingLinkSelection;
use crate::application::use_cases::register_user::RegistrationLinkDecision;
use crate::domain::message::IncomingMessage;
use crate::domain::user::OnboardingState;
use crate::presentation::telegram::active_screens::ScreenDescriptor;
use crate::presentation::telegram::callbacks::TelegramCallback;
use crate::presentation::telegram::ui;

use super::dispatcher_onboarding::render_outcome;
use super::dispatcher_transport::{send_error, send_screen};
use super::TelegramRuntime;

const REGISTRATION_STALE_MESSAGE: &str =
    "Экран привязки уже устарел. Нажмите /start, и я заново покажу варианты.";

pub(crate) async fn handle_registration_link_callback(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: IncomingMessage,
    callback: TelegramCallback,
) -> Result<(), teloxide::RequestError> {
    let chat_id = ChatId(incoming_message.chat_id);

    // Route to onboarding FSM when the user is still in the `AwaitingEmployeeLink`
    // step — the legacy `register_user` path cannot finalise onboarding state.
    if is_user_in_onboarding_link_step(state, incoming_message.sender_id).await {
        return dispatch_to_onboarding(bot, state, incoming_message, callback).await;
    }

    match callback {
        TelegramCallback::RegistrationPickEmployee { employee_id } => {
            let Some(pending) = state.registration_links.get(chat_id.0).await else {
                return send_registration_stale(bot, state, chat_id).await;
            };
            if !pending.candidate_ids.contains(&employee_id) {
                return send_registration_stale(bot, state, chat_id).await;
            }

            match state
                .register_user_use_case
                .execute_with_link_decision(
                    &incoming_message,
                    RegistrationLinkDecision::EmployeeId(employee_id),
                )
                .await
            {
                Ok(actor) => {
                    state.registration_links.clear(chat_id.0).await;
                    send_screen(
                        bot,
                        state,
                        chat_id,
                        ScreenDescriptor::MainMenu,
                        &format!(
                            "✅ Профиль привязан. Теперь задачи будут попадать вам как сотруднику.\n\n{}",
                            ui::welcome_text(&actor)
                        ),
                        ui::main_menu_keyboard(&actor),
                    )
                    .await
                }
                Err(error) => send_error(bot, state, chat_id.0, error).await,
            }
        }
        TelegramCallback::RegistrationContinueUnlinked => match state
            .register_user_use_case
            .execute_with_link_decision(
                &incoming_message,
                RegistrationLinkDecision::ContinueUnlinked,
            )
            .await
        {
            Ok(actor) => {
                state.registration_links.clear(chat_id.0).await;
                send_screen(
                    bot,
                    state,
                    chat_id,
                    ScreenDescriptor::MainMenu,
                    &format!(
                        "ℹ️ Пока продолжаем без привязки к сотруднику. Если появятся назначенные на вас задачи, можно будет вернуться к этому через /start.\n\n{}",
                        ui::welcome_text(&actor)
                    ),
                    ui::main_menu_keyboard(&actor),
                )
                .await
            }
            Err(error) => send_error(bot, state, chat_id.0, error).await,
        },
        _ => Ok(()),
    }
}

async fn is_user_in_onboarding_link_step(state: &TelegramRuntime, telegram_id: i64) -> bool {
    match state
        .onboarding_use_case
        .probe_onboarding_state(telegram_id)
        .await
    {
        Ok(Some(user)) => matches!(user.onboarding_state, OnboardingState::AwaitingEmployeeLink),
        _ => false,
    }
}

async fn dispatch_to_onboarding(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: IncomingMessage,
    callback: TelegramCallback,
) -> Result<(), teloxide::RequestError> {
    let chat_id = ChatId(incoming_message.chat_id);
    let selection = match callback {
        TelegramCallback::RegistrationPickEmployee { employee_id } => {
            OnboardingLinkSelection::PickEmployee(employee_id)
        }
        TelegramCallback::RegistrationContinueUnlinked => {
            OnboardingLinkSelection::ContinueWithoutLink
        }
        _ => return Ok(()),
    };
    let ctx = PrincipalContext::anonymous(
        TelegramChatContext {
            chat_id: incoming_message.chat_id,
            telegram_user_id: incoming_message.sender_id,
        },
        Uuid::new_v4(),
        incoming_message.timestamp,
    );

    match state
        .onboarding_use_case
        .handle_link_selection(&ctx, &incoming_message, selection)
        .await
    {
        Ok(outcome) => {
            state.registration_links.clear(chat_id.0).await;
            let _ = render_outcome(bot, state, chat_id, outcome).await?;
            Ok(())
        }
        Err(error) => send_error(bot, state, chat_id.0, error).await,
    }
}

async fn send_registration_stale(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    send_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::RegistrationLinking,
        REGISTRATION_STALE_MESSAGE,
        InlineKeyboardMarkup::default(),
    )
    .await
}
