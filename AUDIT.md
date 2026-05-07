# Production-Readiness Audit — 2026-05

> Remediation completed: **2026-05-08**
> Branch: `fix/audit-2026-05` → merged to `main`

## Finding Status

| ID   | Severity | Title                                     | Status     | Commit  |
|------|----------|-------------------------------------------|------------|---------|
| F-01 | Critical | Voice transcription not idempotent        | ✅ Fixed   | afe4d59 |
| F-02 | Critical | Callback data 64-byte limit not enforced  | ✅ Fixed   | 9825565 |
| F-03 | Medium   | Admin nonce store — in-memory only        | ✅ Documented | 95d94b1 |
| F-04 | Low      | Empty first_name crashes onboarding       | ✅ Fixed   | a94c152 |
| F-05 | Medium   | No rate-limit on callback presses         | ✅ Fixed   | a94c152 |
| F-06 | Medium   | No Gemini input length cap                | ✅ Fixed   | 95d94b1 |
| F-07 | Low      | Flaky TTL test — real-time sleep          | ✅ Fixed   | a7e4020 |
| F-09 | Medium   | Sheets write-back no exponential backoff  | ✅ Fixed   | 4b77667 |
| M-05 | Minor    | Hardcoded scheduler intervals             | ✅ Fixed   | fd20e59 |

---

## F-01: Voice Transcription Idempotency

**Problem**: `transcribe_voice_message` called OpenAI Whisper on every invocation.
A duplicate Telegram delivery or a user replay would charge the API twice.

**Fix**: Full CAS (Compare-And-Swap) state machine on `voice_processing_records`:

```
MISSING → insert Queued  (INSERT OR IGNORE)
Queued  → Transcribing   (UPDATE WHERE state = 'queued')
Transcribing → Transcribed | Failed
```

Transcript text is stored in the `transcript_text` column (migration 017).
A duplicate call that finds `Transcribed` returns the cached text without
touching OpenAI. A concurrent call that finds `Transcribing` returns
`VOICE_TRANSCRIBING_IN_PROGRESS` to avoid double-charging.

**Tests**: `tests/voice_transcription_idempotency_tests.rs` (3 end-to-end SQLite tests).

---

## F-02: Callback Data 64-Byte Limit

**Problem**: Telegram silently truncates `callback_data` at 64 bytes.
Some encoded callbacks (e.g. `t:open:manager_inbox:UUID:expanded`) exceeded
this limit, causing silent callback failures.

**Fix**:
- `TELEGRAM_CALLBACK_DATA_MAX_BYTES = 64` constant with compile-time assert.
- `encode_callback()` wrapper: if encoded string exceeds 64 bytes it falls
  back to `"m:home"`, logs `tracing::error!`, and increments
  `telegram_callback_overflow_total`.
- Compact origin/mode/status codes (1-2 chars) reduce all known callbacks to
  ≤48 bytes.  Parsers accept both old and new forms (backward compatible).

---

## F-03: Admin Nonce Store — In-Memory Only (Documented)

**Problem**: `AdminNonceStore` stores nonces only in process heap.
On restart or with multiple replicas, nonces from the old process are lost.

**Decision**: **Accepted** for single-instance deployment.

The module doc now explicitly states:
- This is an **intentional skeleton** for single-process deployments.
- Multi-instance migration path: `admin_action_nonces` SQLite table + `dyn AdminNonceRepository` trait.
- Accepted risk: admin must re-click confirm after a restart (minor UX, not security issue).

---

## F-04: Empty first_name Normalization

**Problem**: An empty first_name during onboarding would store a blank
`full_name`, causing broken display everywhere.

**Fix**:
- `sanitize_optional_name`: trims whitespace, returns `None` for empty strings.
- `save_onboarding_progress`: applies sanitization to both first/last name.
- `complete_onboarding`: validates that trimmed `first_name` is non-empty;
  returns `ONBOARDING_NAME_EMPTY` error if not.  Handles trailing space when
  `last_name` is empty.

---

## F-05: Callback Rate-Limit

**Problem**: Inline keyboard presses had no rate-limit, unlike text messages.
A user (or bot) could spam callbacks and trigger expensive DB operations.

**Fix**: `check_callback_rate_limit()` applies the same per-user `TelegramRateLimiter`
to callback presses. On breach, a toast-style `answer_callback_query` is sent
(no new message) and the breach is recorded in the security audit log via the
shared `audit_rate_limit_breach()` helper.

**Exemptions**: `AdminConfirmNonce` and `AdminCancelPending` are always
processed regardless of rate-limit state (the admin cannot "slow down" a
2-step confirmation flow they already started).

---

## F-06: Gemini Input Length Cap

**Problem**: No limit on `task_description` length sent to Gemini.
A verbose voice transcription or prompt-injection attempt could exceed token
limits and inflate costs.

**Fix**: `MAX_GEMINI_INPUT_LENGTH = 8 KiB` constant.  Input is truncated at
a UTF-8 char boundary before building the prompt. On truncation:
- `tracing::warn!` with original/cap byte counts.
- `gemini_input_truncated_total` counter.

Truncated input is still forwarded (not rejected) so legitimate long tasks
are not silently dropped — the system prompt instructs the model to summarise
gracefully.

---

## F-07: Flaky TTL Test

**Problem**: `given_nonce_when_ttl_elapses_then_it_is_expired` used
`thread::sleep(1_100ms)` for a 1-second TTL.  On a loaded CI machine this
was flaky (sleep could complete in <1000ms or extend far beyond 1.1s).

**Fix**: The test now directly backdates the entry's `expires_at` field inside
the mutex, using `Instant::now().checked_sub(Duration::from_secs(120))` with
a fallback for very-fresh-boot systems.  Test now completes in 0 ms and is
fully deterministic.

---

## F-09: Sheets Write-Back — Exponential Backoff

**Problem**: On Sheets API failure, the write-back worker retried every flush
interval (5 min) regardless of how recently the last failure occurred.  During
an outage this would generate up to 20 API calls every 5 minutes.

**Fix**: Migration 018 adds `next_attempt_at TEXT` to `pending_sheet_writes`.
`record_error` computes `next_attempt_at = now + min(2^error_count, 240) minutes`.
`list_pending` filters `next_attempt_at IS NULL OR next_attempt_at <= now`.

When `error_count` reaches `MAX_WRITE_BACK_ATTEMPTS` on its last failure:
- `tracing::error!` with employee details.
- `sheets_write_back_abandoned_total` counter (alertable).

---

## M-05: Configurable Scheduler Intervals

**Problem**: `SLA_CHECK_INTERVAL`, `RECURRENCE_CHECK_INTERVAL`, and
`WRITE_BACK_FLUSH_INTERVAL` were compile-time `const Duration` values.
Operators could not tune them without a code change.

**Fix**: Three new `SchedulerConfig` fields:
- `sla_check_interval_seconds` (env: `SLA_CHECK_INTERVAL_SECONDS`, default: 300)
- `recurrence_check_interval_seconds` (env: `RECURRENCE_CHECK_INTERVAL_SECONDS`, default: 60)
- `write_back_flush_interval_seconds` (env: `WRITE_BACK_FLUSH_INTERVAL_SECONDS`, default: 300)

Defaults are identical to the old hardcoded constants — no behaviour change
on existing deployments.

---

## Architectural Decisions

### ADR-01: In-memory `AdminNonceStore` for single-instance deployment

**Context**: Bot runs as a single Docker container.
**Decision**: In-memory nonce store is acceptable.  The audit doc and code
comment explicitly state the single-instance limitation and the migration path.
**Consequence**: On restart, pending confirmations are lost; admin must re-click.

### ADR-02: Voice transcript caching in SQLite

**Context**: Whisper API costs money.  Duplicate Telegram deliveries are common.
**Decision**: Store `transcript_text` in `voice_processing_records` (migration 017).
Cache hit returns stored text; no Whisper call.
**Consequence**: Old transcripts are retained until `purge_stale_payloads` runs.
`transcript_text` is NULLed by purge to avoid storing PII longer than needed.

### ADR-03: Truncate Gemini input, don't reject

**Context**: Some legitimate task descriptions (pasted meeting notes) exceed 8 KiB.
**Decision**: Truncate silently + log/metric rather than return an error.
The system prompt instructs the model to summarise/refuse gracefully.
**Consequence**: Very long inputs may lose the tail; operators can tune
`MAX_GEMINI_INPUT_LENGTH` if needed (currently a const, not config).
