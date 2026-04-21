#[path = "ui_keyboards.rs"]
mod ui_keyboards;
#[path = "ui_shared.rs"]
mod ui_shared;
#[path = "ui_text.rs"]
mod ui_text;

pub use ui_keyboards::{
    admin_back_keyboard, admin_confirmation_keyboard, admin_features_keyboard, admin_menu_keyboard,
    admin_user_details_keyboard, admin_users_keyboard, cancel_confirmation_keyboard,
    clarification_keyboard, create_menu_keyboard, created_task_followup_keyboard,
    delivery_help_keyboard, describe_pending_admin_action, guided_assignee_keyboard,
    guided_assignee_suggestions_keyboard, guided_confirmation_keyboard, guided_deadline_keyboard,
    main_menu_keyboard, outcome_keyboard, quick_capture_keyboard, registration_link_keyboard,
    task_detail_keyboard, task_list_keyboard, voice_confirmation_keyboard, voice_edit_keyboard,
};
pub use ui_text::{
    admin_access_denied_text, admin_account_deactivated_text, admin_action_cancelled_text,
    admin_audit_text, admin_confirm_text, admin_deactivated_text, admin_features_text,
    admin_last_admin_text, admin_menu_text, admin_nonce_expired_text, admin_nonce_wrong_actor_text,
    admin_reactivated_text, admin_role_changed_text, admin_security_audit_text,
    admin_self_target_text, admin_user_details_text, admin_user_not_found_text, admin_users_text,
    cancel_confirmation_text, create_menu_text, delivery_help_text,
    guided_assignee_clarification_text, guided_assignee_prompt, guided_confirmation_text,
    guided_deadline_prompt, guided_description_prompt, help_text, list_header, list_text,
    onboarding_ask_last_name_text, onboarding_completed_text, onboarding_link_expected_text,
    onboarding_retry_first_name_text, onboarding_retry_last_name_text, onboarding_too_long_text,
    onboarding_welcome_text, quick_create_prompt, registration_link_text, settings_text,
    settings_text_with_stats, stats_text, synced_text, task_blocker_prompt, task_comment_prompt,
    task_creation_text, task_detail_text, task_reassign_prompt, voice_confirmation_text,
    voice_edit_prompt, voice_interpretation_text, welcome_text,
};
