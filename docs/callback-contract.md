# Button and Callback Contract

**Purpose:** Authoritative list of all `TelegramCallback` variants, which Stage accepts each,
whether it mutates state, and what the failure mode is for stale/rejected callbacks.

Callbacks are serialized to a compact binary format (≤ 64 bytes, Telegram limit).
All deserialization failures are handled silently (answer_callback_query with error toast).

---

## Mutating vs. navigational

**Mutating** — changes persistent state (task, user, feature flag). A stale mutating
callback must never execute the mutation. Show an error toast only.

**Navigational** — changes only what screen is displayed. A stale navigational callback
may open a fresh current screen so the user is not stuck.

---

## Callback variant table

### Navigation

| Variant | Mutating | Accepted in Stage(s) | Action |
|---------|:--------:|----------------------|--------|
| `MenuHome` | — | All except Onboarding | Open MainMenu (edit or fresh) |
| `MenuHelp` | — | Main, TaskDetail, Creation | Open Help screen |
| `MenuSettings` | — | Main, TaskDetail | Open Settings |
| `MenuStats` | — | Main | Open personal stats |
| `MenuTeamStats` | — | Main | Open team stats (Manager/Admin) |
| `MenuCreate` | — | Main | Open CreateMenu |
| `MenuSyncEmployees` | — | Main (Admin) | Open sync confirmation |

### Task lists and detail

| Variant | Mutating | Accepted in Stage(s) | Action |
|---------|:--------:|----------------------|--------|
| `ListTasks { origin, cursor }` | — | Main, TaskDetail, Creation | Open task list for origin |
| `OpenTask { task_uid, origin, mode }` | — | Main, TaskDetail, Creation | Open task card |
| `ConfirmTaskCancel { task_uid, origin }` | — | TaskDetail | Open cancel confirmation |

### Task mutations

| Variant | Mutating | Accepted in Stage(s) | Action |
|---------|:--------:|----------------------|--------|
| `UpdateTaskStatus { task_uid, next_status, origin }` | ✓ | TaskDetail | Change task status |
| `ExecuteTaskCancel { task_uid, origin }` | ✓ | TaskDetail | Finalize cancellation |
| `ShowDeliveryHelp { task_uid, origin }` | — | TaskDetail | Open delivery help modal |
| `StartTaskCommentInput { task_uid, origin }` | — | TaskDetail | Enter comment mode |
| `StartTaskBlockerInput { task_uid, origin }` | — | TaskDetail | Enter blocker mode |
| `StartTaskReassignInput { task_uid, origin }` | — | TaskDetail | Enter reassignment mode |

### Creation

| Variant | Mutating | Accepted in Stage(s) | Action |
|---------|:--------:|----------------------|--------|
| `StartQuickCreate` | — | Main, Creation | Start quick-capture session |
| `StartGuidedCreate` | — | Main, Creation | Start guided create flow |
| `DraftSkipAssignee` | — | Creation | Advance guided flow without assignee |
| `DraftSkipDeadline` | — | Creation | Advance guided flow without deadline |
| `DraftEdit { field }` | — | Creation | Return to field for editing |
| `DraftSubmit` | ✓ | Creation | Create task from guided draft |
| `GuidedAssigneeConfirm { employee_id }` | ✓ | Creation | Confirm resolved employee (assigns draft) |
| `VoiceCreateConfirm` | ✓ | Creation | Create task from voice transcript |
| `VoiceCreateEdit` | — | Creation | Enter transcript edit mode |
| `VoiceCreateBack` | — | Creation | Return to CreateMenu |
| `VoiceCreateCancel` | — | Creation | Abort voice session, return to CreateMenu |

### Assignee clarification

| Variant | Mutating | Accepted in Stage(s) | Action |
|---------|:--------:|----------------------|--------|
| `ClarificationPickEmployee { employee_id }` | ✓ | Creation | Assign selected employee; advance flow |
| `ClarificationCreateUnassigned` | ✓ | Creation | Create task without assignee |

### Registration and onboarding

| Variant | Mutating | Accepted in Stage(s) | Action |
|---------|:--------:|----------------------|--------|
| `RegistrationPickEmployee { employee_id }` | ✓ | Registration | Link Telegram account to employee |
| `RegistrationContinueUnlinked` | ✓ | Registration | Register without employee link |

### Admin panel

| Variant | Mutating | Accepted in Stage(s) | Action |
|---------|:--------:|----------------------|--------|
| `AdminMenu` | — | Admin, Main | Open admin home |
| `AdminUsers` | — | Admin | List all users |
| `AdminUserDetails { user_id }` | — | Admin | View user details |
| `AdminUserPrepareRoleChange { user_id, next_role }` | — | Admin | Request role-change nonce |
| `AdminUserPrepareDeactivate { user_id }` | — | Admin | Request deactivation nonce |
| `AdminUserPrepareReactivate { user_id }` | — | Admin | Request reactivation nonce |
| `AdminConfirmNonce { nonce }` | ✓ | Admin | Execute nonce-protected mutation |
| `AdminCancelPending` | — | Admin | Cancel pending nonce |
| `AdminAudit` | — | Admin | Open mutation audit log |
| `AdminSecurityAudit` | — | Admin | Open security audit log |
| `AdminFeatures` | — | Admin | Open feature flag editor |
| `AdminToggleFeature { flag_key }` | ✓ | Admin | Toggle feature flag |

---

## Cross-cutting rules

### task_uid isolation
Callbacks carrying a `task_uid` (UpdateTaskStatus, ExecuteTaskCancel, etc.) must
verify that the `task_uid` matches the active TaskDetail screen's `task_uid`.
Mismatches are treated as stale and rejected safely.

### Codec roundtrip
Every callback variant must survive a serialize → deserialize roundtrip without
data loss. This is a required unit test for each variant.

### Forbidden buttons
A button must never be rendered if the user's role does not permit the corresponding
action. The application layer independently re-checks authorization, but the UI must
not present forbidden options in the first place.

### Duplicate-tap safety
All mutating callbacks are idempotency-safe either via:
- `source_message_key` dedup (task creation), or
- `submission_key: Uuid` on guided drafts, or
- Nonce single-use (admin actions).
A second tap within the same session is safe to repeat.
