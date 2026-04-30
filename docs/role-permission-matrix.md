# Role and Permission Matrix

**Three roles:** `User`, `Manager`, `Admin`.

All role checks exist at two levels:
1. **Presentation layer** — buttons are only shown when the action is permitted.
2. **Application layer** — `RoleAuthorizationPolicy` enforces in use cases, independent of UI.

A deactivated user (`deactivated_at IS NOT NULL`) is denied ALL actions regardless of role.
This is checked first by `ensure_active()` before any other policy method.

---

## System-level permissions

| Action | User | Manager | Admin |
|--------|:----:|:-------:|:-----:|
| Register / onboard | ✓ | ✓ | ✓ |
| View own tasks | ✓ | ✓ | ✓ |
| View team tasks | — | ✓ | ✓ |
| View team statistics | — | ✓ | ✓ |
| View manager inbox | — | ✓ | ✓ |
| Trigger employee sync | — | — | ✓ |
| Access admin panel | — | — | ✓ |
| Toggle feature flags | — | — | ✓ |
| Change user roles | — | — | ✓ |
| Deactivate/reactivate users | — | — | ✓ |
| View audit log | — | — | ✓ |
| View security audit log | — | — | ✓ |

**Last-admin invariant:** Admin cannot change their own role or deactivate themselves.
`ensure_not_self()` enforces this; the last admin is always protected.

---

## Task-level permissions

### Who can view a task
- Creator of the task.
- Assignee of the task.
- Any Manager or Admin (sees all tasks).

### Who can comment
- Creator, assignee, Manager, Admin.
- No other users.

### Who can report a blocker
- Assignee only (the person blocked owns the report).
- Manager and Admin can override by writing directly.

### Who can reassign
- Creator.
- Assignee.
- Manager or Admin.

### Status transition permissions

| From → To | User (creator) | User (assignee) | Manager | Admin |
|-----------|:--------------:|:---------------:|:-------:|:-----:|
| Any → Created | — | — | — | — |
| Created → Sent | ✓ | — | ✓ | ✓ |
| Created/Sent → InProgress | — | ✓ | ✓ | ✓ |
| Any → Blocked | — | ✓ | ✓ | ✓ |
| Any → InReview | — | ✓ | ✓ | ✓ |
| InReview → InProgress | ✓ | — | ✓ | ✓ |
| InReview → Completed | ✓ | see note | ✓ | ✓ |
| Any → Cancelled | ✓ | ✓ | ✓ | ✓ |

> **Review gate note:** If the task has `expected_result` set, an assignee requesting
> `Completed` is silently redirected to `InReview`. The creator or admin must then
> approve `InReview → Completed`.

---

## Admin 2-step confirmation

Destructive admin actions (role change, deactivate, reactivate) use a nonce flow:
1. Admin taps the action button → bot creates a one-time nonce + shows confirm screen.
2. Admin taps "✅ Подтвердить" with the nonce.
3. Use case validates nonce (single-use, time-limited) → executes mutation.

This prevents misclicks on destructive operations.
