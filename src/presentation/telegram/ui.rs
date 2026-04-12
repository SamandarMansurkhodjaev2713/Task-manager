#[path = "ui_keyboards.rs"]
mod ui_keyboards;
#[path = "ui_shared.rs"]
mod ui_shared;
#[path = "ui_text.rs"]
mod ui_text;

pub use ui_keyboards::{
    cancel_confirmation_keyboard, create_menu_keyboard, guided_assignee_keyboard,
    guided_confirmation_keyboard, guided_deadline_keyboard, main_menu_keyboard, outcome_keyboard,
    task_detail_keyboard, task_list_keyboard,
};
pub use ui_text::{
    cancel_confirmation_text, create_menu_text, guided_assignee_prompt, guided_confirmation_text,
    guided_deadline_prompt, guided_description_prompt, help_text, list_header, list_text,
    quick_create_prompt, settings_text, stats_text, synced_text, task_blocker_prompt,
    task_comment_prompt, task_creation_text, task_detail_text, task_reassign_prompt, welcome_text,
};
