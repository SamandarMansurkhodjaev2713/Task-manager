use teloxide::types::{ChatId, InlineKeyboardMarkup};
use teloxide::Bot;

use crate::application::use_cases::register_user::RegistrationLinkDecision;
use crate::domain::message::IncomingMessage;
use crate::presentation::telegram::active_screens::ScreenDescriptor;
use crate::presentation::telegram::callbacks::TelegramCallback;
use crate::presentation::telegram::ui;

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
                Err(error) => send_error(bot, chat_id.0, error).await,
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
            Err(error) => send_error(bot, chat_id.0, error).await,
        },
        _ => Ok(()),
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
