//! Integration tests for the `ScreenDescriptor`/`Stage` capability matrix
//! introduced as part of P0 "active-screen hardening".  We duplicate the
//! essentials of the unit tests here because the local AppControl policy
//! blocks the library test binary — the integration harness runs fine.

use telegram_task_bot::presentation::telegram::active_screens::{ScreenDescriptor, Stage};
use telegram_task_bot::presentation::telegram::callbacks::{
    TaskCardMode, TaskListOrigin, TelegramCallback,
};
use uuid::Uuid;

#[test]
fn given_onboarding_stage_when_main_menu_navigation_callback_then_rejected() {
    let screen = ScreenDescriptor::OnboardingFirstName;
    assert_eq!(screen.stage(), Stage::Onboarding);

    // Every main-menu navigation is refused while onboarding is active.
    // This closes hypothesis (a) from the P0 screenshot analysis: a stale
    // "Мой фокус" button from the welcome screen must no longer open the
    // focus list between onboarding steps.
    assert!(!screen.accepts(&TelegramCallback::MenuHome));
    assert!(!screen.accepts(&TelegramCallback::MenuCreate));
    assert!(!screen.accepts(&TelegramCallback::MenuStats));
    assert!(!screen.accepts(&TelegramCallback::ListTasks {
        origin: TaskListOrigin::Focus,
        cursor: None,
    }));
}

#[test]
fn given_task_detail_when_cancel_callback_targets_another_task_then_rejected() {
    let active = Uuid::now_v7();
    let other = Uuid::now_v7();
    let screen = ScreenDescriptor::TaskDetail {
        task_uid: active,
        mode: TaskCardMode::Compact,
        origin: TaskListOrigin::Created,
    };

    assert!(!screen.accepts(&TelegramCallback::ExecuteTaskCancel {
        task_uid: other,
        origin: TaskListOrigin::Created,
    }));
    assert!(screen.accepts(&TelegramCallback::ExecuteTaskCancel {
        task_uid: active,
        origin: TaskListOrigin::Created,
    }));
}

#[test]
fn given_main_screen_when_creation_entrypoint_pressed_then_accepted() {
    let main = ScreenDescriptor::MainMenu;
    assert!(main.accepts(&TelegramCallback::StartQuickCreate));
    assert!(main.accepts(&TelegramCallback::StartGuidedCreate));
    assert!(main.accepts(&TelegramCallback::MenuCreate));
}

#[test]
fn given_admin_menu_when_foreign_stage_callback_then_rejected() {
    let admin = ScreenDescriptor::AdminMenu;
    assert!(admin.accepts(&TelegramCallback::AdminUsers));
    assert!(!admin.accepts(&TelegramCallback::StartQuickCreate));
    assert!(!admin.accepts(&TelegramCallback::VoiceCreateConfirm));
}
