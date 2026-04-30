//! Telegram Inline Mode handler.
//!
//! When a user types `@bot_name <query>` in any Telegram chat, the bot
//! receives an [`InlineQuery`] update and must respond with a list of
//! [`InlineQueryResult`] items within a few seconds.
//!
//! We use the inline mode to let users quickly look up employees when
//! assigning tasks directly from any chat — particularly useful when
//! forwarding a message or constructing a task from a group chat.
//!
//! # Result format
//!
//! Each result is an `Article` with:
//! - **title**: `"Иван Иванов"` (full name)
//! - **description**: `"@ivanov"` (username) or department
//! - **input_message_content**: the text `"Иван Иванов"` that the bot
//!   can parse as an assignee name in subsequent task-creation flows.
//!
//! Up to `MAX_INLINE_RESULTS` candidates are returned, sorted by match
//! confidence descending.

use std::sync::Arc;

use teloxide::payloads::AnswerInlineQuerySetters;
use teloxide::prelude::Requester;
use teloxide::types::{
    InlineQuery, InlineQueryResult, InlineQueryResultArticle, InputMessageContent,
    InputMessageContentText,
};
use teloxide::Bot;

use super::TelegramRuntime;

/// Maximum number of candidates returned in a single inline response.
const MAX_INLINE_RESULTS: usize = 8;

pub(super) async fn handle_inline_query(
    bot: Bot,
    query: InlineQuery,
    state: Arc<TelegramRuntime>,
) -> Result<(), teloxide::RequestError> {
    let raw_query = query.query.trim();

    // Empty query: return a short help article so the user sees something.
    if raw_query.is_empty() {
        let placeholder = InlineQueryResult::Article(
            InlineQueryResultArticle::new(
                "help",
                "Введите имя сотрудника…",
                InputMessageContent::Text(InputMessageContentText::new(
                    "Начните вводить имя или ник сотрудника для поиска.",
                )),
            )
            .description("Поиск работает по именам, псевдонимам и @username"),
        );
        bot.answer_inline_query(&query.id, vec![placeholder])
            .cache_time(0)
            .await?;
        return Ok(());
    }

    let candidates = state
        .assignee_resolver
        .search_employees(raw_query, MAX_INLINE_RESULTS)
        .await
        .unwrap_or_default();

    if candidates.is_empty() {
        let not_found = InlineQueryResult::Article(
            InlineQueryResultArticle::new(
                "not_found",
                "Сотрудник не найден",
                InputMessageContent::Text(InputMessageContentText::new(format!(
                    "Не нашёл сотрудника по запросу «{raw_query}»."
                ))),
            )
            .description("Попробуйте другой вариант написания"),
        );
        bot.answer_inline_query(&query.id, vec![not_found])
            .cache_time(0)
            .await?;
        return Ok(());
    }

    let results: Vec<InlineQueryResult> = candidates
        .iter()
        .map(|candidate| {
            let employee = &candidate.employee;
            let id = employee
                .id
                .map(|id| id.to_string())
                .unwrap_or_else(|| candidate.employee.full_name.clone());

            let description = match &employee.telegram_username {
                Some(username) => format!("@{username}"),
                None => employee
                    .department
                    .clone()
                    .unwrap_or_else(|| "Сотрудник".to_owned()),
            };

            InlineQueryResult::Article(
                InlineQueryResultArticle::new(
                    id,
                    &employee.full_name,
                    // The input text is the employee's full name so the bot
                    // can parse it as an assignee in a subsequent task message.
                    InputMessageContent::Text(InputMessageContentText::new(&employee.full_name)),
                )
                .description(description),
            )
        })
        .collect();

    // Cache for 30 seconds: short enough to reflect employee sync updates,
    // long enough to avoid hammering the DB on fast typists.
    bot.answer_inline_query(&query.id, results)
        .cache_time(30)
        .await?;
    Ok(())
}
