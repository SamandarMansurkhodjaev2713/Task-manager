#[path = "ui_text_lists.rs"]
mod ui_text_lists;
#[path = "ui_text_menu.rs"]
mod ui_text_menu;
#[path = "ui_text_task.rs"]
mod ui_text_task;

pub use ui_text_lists::{list_header, list_text, task_creation_text};
pub use ui_text_menu::{
    create_menu_text, guided_assignee_prompt, guided_confirmation_text, guided_deadline_prompt,
    guided_description_prompt, help_text, quick_create_prompt, settings_text, stats_text,
    synced_text, welcome_text,
};
pub use ui_text_task::{
    cancel_confirmation_text, task_blocker_prompt, task_comment_prompt, task_detail_text,
    task_reassign_prompt,
};
