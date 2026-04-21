use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use uuid::Uuid;

use crate::application::use_cases::collect_stats::StatsScope;
use crate::presentation::telegram::callbacks::{TaskCardMode, TaskListOrigin, TelegramCallback};
use crate::presentation::telegram::drafts::{GuidedTaskStep, VoiceTaskStep};
use crate::presentation::telegram::interactions::TaskInteractionKind;

/// Top-level UX stage the user is currently in.  Derived from
/// [`ScreenDescriptor`] and used as the source of truth for callback
/// acceptance — see [`Stage::accepts_callback`].
///
/// Stages are **mutually exclusive**: a user is either still being onboarded
/// or they are navigating the main product or interacting with a creation
/// wizard, etc.  Each stage has a small, auditable set of allowed callbacks;
/// anything else is rejected *before* reaching the business handler.  This
/// replaces the previous 80-line `callback_matches_screen` match statement in
/// the dispatcher, which grew monotonically with each new feature and had
/// become impossible to review for leaks (e.g. "is navigation really allowed
/// during onboarding?").
///
/// **Onboarding stage is deliberately hermetic**: no main-menu navigation,
/// no task list, no admin.  The only accepted callbacks are the ones owned
/// by the onboarding FSM itself.  This closes the "Мой фокус between steps"
/// hypothesis from the P0 screenshot analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Unknown,
    Registration,
    Onboarding,
    Main,
    Creation,
    TaskDetail,
    Admin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenDescriptor {
    Unknown,
    /// Transient screen shown while a slow async operation (e.g. voice transcription) is running.
    /// Always replaced by the next real screen — never displayed with interactive buttons.
    Processing,
    RegistrationLinking,
    /// Onboarding v2 — "enter your first name" step.
    OnboardingFirstName,
    /// Onboarding v2 — "enter your last name" step.
    OnboardingLastName,
    MainMenu,
    Help,
    Settings,
    CreateMenu,
    QuickCreate,
    GuidedStep(GuidedTaskStep),
    /// Shown during the guided Assignee step when the entered text is ambiguous
    /// or only partially matched — presents candidate employees for the user to
    /// pick before advancing to Description.
    GuidedAssigneeOptions,
    VoiceCreate(VoiceTaskStep),
    TaskList(TaskListOrigin),
    TaskDetail {
        task_uid: Uuid,
        mode: TaskCardMode,
        origin: TaskListOrigin,
    },
    CancelConfirmation {
        task_uid: Uuid,
        origin: TaskListOrigin,
    },
    TaskInteractionPrompt {
        task_uid: Uuid,
        origin: TaskListOrigin,
        kind: TaskInteractionKind,
    },
    TaskCreationResult {
        task_uid: Option<Uuid>,
    },
    DeliveryHelp {
        task_uid: Uuid,
        origin: TaskListOrigin,
    },
    Stats(StatsScope),
    SyncEmployeesResult,
    // ── Admin panel screens (Phase 4) ────────────────────────────────────
    AdminMenu,
    AdminUsers,
    AdminUserDetails {
        user_id: i64,
    },
    /// Two-step confirmation for a destructive admin action.  Stores the
    /// nonce so that a stale confirmation callback (pressed after the user
    /// navigated elsewhere) can be detected and ignored.
    AdminConfirm {
        nonce: String,
    },
    AdminAudit,
    AdminSecurityAudit,
    AdminFeatures,
}

impl ScreenDescriptor {
    /// Classify the descriptor into one of the UX stages.  Used to derive
    /// callback acceptance in a single place (see [`Stage::accepts_callback`])
    /// rather than repeating exhaustive matches across the dispatcher.
    pub fn stage(&self) -> Stage {
        match self {
            Self::Unknown => Stage::Unknown,
            Self::Processing => Stage::Unknown,
            Self::RegistrationLinking => Stage::Registration,
            Self::OnboardingFirstName | Self::OnboardingLastName => Stage::Onboarding,
            Self::MainMenu
            | Self::Help
            | Self::Settings
            | Self::Stats(_)
            | Self::SyncEmployeesResult
            | Self::TaskList(_) => Stage::Main,
            Self::CreateMenu
            | Self::QuickCreate
            | Self::GuidedStep(_)
            | Self::GuidedAssigneeOptions
            | Self::VoiceCreate(_)
            | Self::TaskCreationResult { .. }
            | Self::TaskInteractionPrompt { .. } => Stage::Creation,
            Self::TaskDetail { .. }
            | Self::CancelConfirmation { .. }
            | Self::DeliveryHelp { .. } => Stage::TaskDetail,
            Self::AdminMenu
            | Self::AdminUsers
            | Self::AdminUserDetails { .. }
            | Self::AdminConfirm { .. }
            | Self::AdminAudit
            | Self::AdminSecurityAudit
            | Self::AdminFeatures => Stage::Admin,
        }
    }

    /// Returns `true` if the given callback is allowed against this screen.
    /// The default is derived from [`Stage::accepts_callback`] and can be
    /// overridden here when a screen needs finer-grained rules (e.g. task
    /// detail callbacks must target the *same* task_uid as the active card).
    pub fn accepts(&self, callback: &TelegramCallback) -> bool {
        match (self, callback) {
            // TaskDetail is stricter than the stage default: the task_uid
            // inside the callback MUST match the active card or the cancel
            // confirmation for that card.
            (
                Self::TaskDetail {
                    task_uid: active_uid,
                    ..
                }
                | Self::CancelConfirmation {
                    task_uid: active_uid,
                    ..
                },
                TelegramCallback::UpdateTaskStatus { task_uid, .. }
                | TelegramCallback::ConfirmTaskCancel { task_uid, .. }
                | TelegramCallback::ExecuteTaskCancel { task_uid, .. }
                | TelegramCallback::StartTaskCommentInput { task_uid, .. }
                | TelegramCallback::StartTaskBlockerInput { task_uid, .. }
                | TelegramCallback::StartTaskReassignInput { task_uid, .. }
                | TelegramCallback::ShowDeliveryHelp { task_uid, .. },
            ) => active_uid == task_uid,

            // Registration clarifications are allowed only while we actually
            // show the registration-linking screen.
            (Self::RegistrationLinking, TelegramCallback::RegistrationPickEmployee { .. })
            | (Self::RegistrationLinking, TelegramCallback::RegistrationContinueUnlinked) => true,

            // Everything else: fall back to the stage default.
            _ => self.stage().accepts_callback(callback),
        }
    }
}

impl Stage {
    /// Capability matrix: which callbacks does this stage accept?  Every
    /// branch is explicit — no wildcards — so adding a new callback without
    /// updating this function produces a compilation error (`non-exhaustive
    /// patterns`), forcing the author to decide where it belongs.
    pub fn accepts_callback(self, callback: &TelegramCallback) -> bool {
        use TelegramCallback as CB;

        match self {
            // Onboarding is hermetic: nothing from the outer product may
            // reach the FSM.  The only legal callbacks are registration
            // clarifications (handled by the ScreenDescriptor override) and
            // nothing else — free-text navigation is how the user moves here.
            Stage::Onboarding => false,

            // Registration clarification screen.  The override on
            // ScreenDescriptor::RegistrationLinking accepts the two
            // registration buttons; everything else is rejected.
            Stage::Registration => false,

            Stage::Main => matches!(
                callback,
                CB::MenuHome
                    | CB::MenuHelp
                    | CB::MenuSettings
                    | CB::MenuStats
                    | CB::MenuTeamStats
                    | CB::MenuCreate
                    | CB::MenuSyncEmployees
                    | CB::ListTasks { .. }
                    | CB::OpenTask { .. }
                    // Creation entrypoints are reachable from the main menu
                    // (e.g. the "✏️ Создать задачу" buttons in the menu and
                    // the "task created" result card both live under
                    // Stage::Main / Stage::Creation — we allow both).
                    | CB::StartQuickCreate
                    | CB::StartGuidedCreate
                    | CB::AdminMenu
            ),

            Stage::Creation => matches!(
                callback,
                CB::StartQuickCreate
                    | CB::StartGuidedCreate
                    | CB::VoiceCreateConfirm
                    | CB::VoiceCreateEdit
                    | CB::VoiceCreateBack
                    | CB::VoiceCreateCancel
                    | CB::DraftSkipAssignee
                    | CB::DraftSkipDeadline
                    | CB::DraftSubmit
                    | CB::DraftEdit { .. }
                    | CB::ClarificationPickEmployee { .. }
                    | CB::ClarificationCreateUnassigned
                    | CB::GuidedAssigneeConfirm { .. }
                    | CB::MenuHome
                    | CB::OpenTask { .. }
            ),

            Stage::TaskDetail => matches!(
                callback,
                CB::MenuHome
                    | CB::MenuHelp
                    | CB::ListTasks { .. }
                    | CB::OpenTask { .. }
                    | CB::UpdateTaskStatus { .. }
                    | CB::ConfirmTaskCancel { .. }
                    | CB::ExecuteTaskCancel { .. }
                    | CB::StartTaskCommentInput { .. }
                    | CB::StartTaskBlockerInput { .. }
                    | CB::StartTaskReassignInput { .. }
                    | CB::ShowDeliveryHelp { .. }
            ),

            Stage::Admin => matches!(
                callback,
                CB::AdminMenu
                    | CB::AdminUsers
                    | CB::AdminUserDetails { .. }
                    | CB::AdminUserPrepareRoleChange { .. }
                    | CB::AdminUserPrepareDeactivate { .. }
                    | CB::AdminUserPrepareReactivate { .. }
                    | CB::AdminConfirmNonce { .. }
                    | CB::AdminCancelPending
                    | CB::AdminAudit
                    | CB::AdminSecurityAudit
                    | CB::AdminFeatures
                    | CB::AdminToggleFeature { .. }
                    | CB::MenuHome
            ),

            // Unknown stage: be permissive for backwards compatibility with
            // legacy sessions that pre-date stage tracking.  The active
            // screens store will be hydrated into a real stage on the next
            // fresh render.
            Stage::Unknown => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveScreenState {
    pub message_id: i32,
    pub descriptor: ScreenDescriptor,
}

#[derive(Clone, Default)]
pub struct ActiveScreenStore {
    states: Arc<RwLock<HashMap<i64, ActiveScreenState>>>,
}

impl ActiveScreenStore {
    pub async fn get(&self, chat_id: i64) -> Option<ActiveScreenState> {
        self.states.read().await.get(&chat_id).cloned()
    }

    pub async fn set(&self, chat_id: i64, state: ActiveScreenState) {
        self.states.write().await.insert(chat_id, state);
    }

    pub async fn clear(&self, chat_id: i64) {
        self.states.write().await.remove(&chat_id);
    }

    pub async fn hydrate_if_missing(&self, chat_id: i64, message_id: i32) {
        let mut states = self.states.write().await;
        states.entry(chat_id).or_insert(ActiveScreenState {
            message_id,
            descriptor: ScreenDescriptor::Unknown,
        });
    }

    pub async fn is_stale(&self, chat_id: i64, message_id: i32) -> bool {
        self.get(chat_id)
            .await
            .map(|state| state.message_id != message_id)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::{ActiveScreenState, ActiveScreenStore, ScreenDescriptor, Stage};
    use crate::presentation::telegram::callbacks::{
        AdminRoleOption, TaskCardMode, TaskListOrigin, TelegramCallback,
    };
    use uuid::Uuid;

    #[tokio::test]
    async fn given_missing_state_when_hydrate_then_registers_unknown_screen() {
        let store = ActiveScreenStore::default();

        store.hydrate_if_missing(42, 100).await;

        assert_eq!(
            store.get(42).await,
            Some(ActiveScreenState {
                message_id: 100,
                descriptor: ScreenDescriptor::Unknown,
            })
        );
    }

    #[tokio::test]
    async fn given_current_screen_when_message_id_differs_then_detects_stale_state() {
        let store = ActiveScreenStore::default();
        store
            .set(
                7,
                ActiveScreenState {
                    message_id: 200,
                    descriptor: ScreenDescriptor::MainMenu,
                },
            )
            .await;

        assert!(store.is_stale(7, 150).await);
        assert!(!store.is_stale(7, 200).await);
    }

    #[test]
    fn given_onboarding_stage_when_navigation_callback_arrives_then_it_is_rejected() {
        // GIVEN: user is still entering their first name.
        let screen = ScreenDescriptor::OnboardingFirstName;
        assert_eq!(screen.stage(), Stage::Onboarding);

        // WHEN: they press a stale "Мой фокус" button that belongs to the
        //       main menu (the bug we saw on the P0 screenshot).
        let navigation_to_focus = TelegramCallback::ListTasks {
            origin: TaskListOrigin::Focus,
            cursor: None,
        };

        // THEN: the capability matrix refuses it.  The dispatcher will
        //       answer a toast and re-render the current onboarding step.
        assert!(!screen.accepts(&navigation_to_focus));
        assert!(!screen.accepts(&TelegramCallback::MenuHome));
        assert!(!screen.accepts(&TelegramCallback::MenuCreate));
    }

    #[test]
    fn given_task_detail_when_callback_targets_other_task_then_it_is_rejected() {
        let active_uid = Uuid::now_v7();
        let other_uid = Uuid::now_v7();
        let screen = ScreenDescriptor::TaskDetail {
            task_uid: active_uid,
            mode: TaskCardMode::Compact,
            origin: TaskListOrigin::Created,
        };

        let cancel_other = TelegramCallback::ExecuteTaskCancel {
            task_uid: other_uid,
            origin: TaskListOrigin::Created,
        };

        assert!(
            !screen.accepts(&cancel_other),
            "destructive callbacks must not leak across task cards"
        );
    }

    #[test]
    fn given_admin_stage_when_regular_navigation_callback_arrives_then_it_is_rejected() {
        let screen = ScreenDescriptor::AdminMenu;
        assert_eq!(screen.stage(), Stage::Admin);

        assert!(screen.accepts(&TelegramCallback::AdminUsers));
        assert!(screen.accepts(&TelegramCallback::MenuHome));
        assert!(!screen.accepts(&TelegramCallback::MenuCreate));
        assert!(!screen.accepts(&TelegramCallback::StartQuickCreate));
    }

    #[test]
    fn given_main_stage_when_creation_entry_point_arrives_then_it_is_accepted() {
        let main = ScreenDescriptor::MainMenu;
        assert_eq!(main.stage(), Stage::Main);
        assert!(main.accepts(&TelegramCallback::StartQuickCreate));
        assert!(main.accepts(&TelegramCallback::StartGuidedCreate));
        assert!(main.accepts(&TelegramCallback::MenuCreate));
        assert!(main.accepts(&TelegramCallback::MenuStats));
    }

    #[test]
    fn given_registration_screen_when_pick_employee_then_it_is_accepted() {
        let screen = ScreenDescriptor::RegistrationLinking;
        assert!(screen.accepts(&TelegramCallback::RegistrationContinueUnlinked));
        assert!(screen.accepts(&TelegramCallback::RegistrationPickEmployee { employee_id: 7 }));
        assert!(
            !screen.accepts(&TelegramCallback::AdminUserPrepareRoleChange {
                user_id: 1,
                next_role: AdminRoleOption::Admin,
            }),
            "admin mutations must not cross into the registration screen"
        );
    }
}
