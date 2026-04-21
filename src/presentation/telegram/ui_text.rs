#[path = "ui_text_admin.rs"]
mod ui_text_admin;
#[path = "ui_text_lists.rs"]
mod ui_text_lists;
#[path = "ui_text_menu.rs"]
mod ui_text_menu;
#[path = "ui_text_task.rs"]
mod ui_text_task;

pub use ui_text_admin::{
    admin_access_denied_text, admin_account_deactivated_text, admin_action_cancelled_text,
    admin_audit_text, admin_confirm_text, admin_deactivated_text, admin_features_text,
    admin_last_admin_text, admin_menu_text, admin_nonce_expired_text, admin_nonce_wrong_actor_text,
    admin_reactivated_text, admin_role_changed_text, admin_security_audit_text,
    admin_self_target_text, admin_user_details_text, admin_user_not_found_text, admin_users_text,
};
pub use ui_text_lists::{list_header, list_text, task_creation_text};
pub use ui_text_menu::{
    create_menu_text, guided_assignee_clarification_text, guided_assignee_prompt,
    guided_confirmation_text, guided_deadline_prompt, guided_description_prompt, help_text,
    onboarding_ask_last_name_text, onboarding_completed_text, onboarding_link_expected_text,
    onboarding_retry_first_name_text, onboarding_retry_last_name_text, onboarding_too_long_text,
    onboarding_welcome_text, quick_create_prompt, registration_link_text, settings_text,
    settings_text_with_stats, stats_text, synced_text, voice_confirmation_text, voice_edit_prompt,
    voice_interpretation_text, welcome_text,
};
pub use ui_text_task::{
    cancel_confirmation_text, delivery_help_text, task_blocker_prompt, task_comment_prompt,
    task_detail_text, task_reassign_prompt,
};
