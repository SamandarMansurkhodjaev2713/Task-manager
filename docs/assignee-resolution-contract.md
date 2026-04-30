# Assignee Search and Resolution Contract

**Purpose:** Authoritative specification for how the bot resolves a name string (from text
input, voice transcript, or guided step) to a concrete `Employee`. All implementation
changes to the resolver must be reflected here.

---

## Design principles

1. **Resolution is layered.** Each layer is tried in priority order; the first conclusive
   match wins. Lower layers are fallback, not "also run in parallel."
2. **Silent assignment only for deterministic exact matches.** Any fuzzy or ambiguous
   result requires an explicit user confirmation tap before a task is created with that
   assignee. The bot never silently creates a wrongly-assigned task.
3. **No-match is a recoverable state.** When all layers return nothing, the user is offered
   unassigned creation (where permitted) or an explicit search/retry path — never a dead end.
4. **Same pipeline for all entry points.** Text input, voice transcript, and guided step
   all call the same resolver chain. No separate "voice matching" logic.
5. **Both employee sources are equal candidates.** `google_sheets` and `bot_registered`
   employees appear in all candidate lists and confidence calculations alike.

---

## Resolver chain (current implementation)

Executed in order; first conclusive match terminates the chain.

| # | Layer | Input type | Confidence | Outcome type |
|---|-------|-----------|-----------|-------------|
| 1 | Exact `@username` match | `@handle` | 100 | Unique or Ambiguous |
| 2 | Exact full-name match (normalized) | multi-word | 100 | Unique or Ambiguous |
| 3 | Exact first-name match | single word | 100 | Unique or Ambiguous |
| 4 | Prefix on first name (≥ 2 chars) | single word | 78 | Unique or Ambiguous |
| 5 | Levenshtein fuzzy on first name | single word | 60–94 | Suggested or Ambiguous |
| 6 | Levenshtein fuzzy on full name | multi-word | 60–94 | Suggested or Ambiguous |

**Normalization rules (applied before all string comparisons):**
- Lowercase.
- `ё` → `е`.
- Collapse internal whitespace.
- Strip leading `@` before username comparison.

---

## Outcome types and UX

| Outcome | Definition | UI behavior |
|---------|-----------|------------|
| `Unique` | Exactly one candidate at confidence ≥ 95 | Auto-assign; no confirmation needed |
| `Suggested` | One top candidate at 75–94 + optional alternatives | "Вы имели в виду — X?" with confirm button + show alternatives |
| `Ambiguous` | Multiple candidates all at ≥ 75 | Candidate keyboard; user picks one |
| `NotFound` | All candidates < 75 | Unassigned fallback (if allowed) or explicit retry |

---

## Confidence thresholds

| Constant | Value | Used for |
|----------|-------|---------|
| `HIGH_CONFIDENCE_THRESHOLD` | 95 | Auto-assign without prompt |
| `SUGGESTED_CONFIDENCE_THRESHOLD` | 75 | Minimum to appear as a suggestion |
| `PREFIX_MATCH_CONFIDENCE` | 78 | Prefix-match results |

Minimum number of candidates returned for Ambiguous: up to 3, sorted by confidence
desc then full_name asc.

---

## Person trigrams (not yet in production path)

The `person_trigrams` table and its index exist (migration 009) and are populated from
employee names. They are **not currently queried** in the production resolution chain.
The current fuzzy layer uses in-memory Levenshtein over the employees already loaded.

**Planned use:** Phase 2 introduces a trigram-backed DB query as layer 5.5, between
Levenshtein and no-match, giving better CJK/typo resistance and enabling `/find` to
search task text without a full-scan.

---

## Registration-time matching (different policy)

`employee_matching.rs` uses a stricter policy deliberately:

1. Exact `@username` → if one match, auto-link; if multiple, show choice.
2. Exact full name (normalized) → if one match, auto-link; if multiple, show choice.
3. No match → register without link; can link later.

**No fuzzy at registration.** Rationale: wrong employee link at onboarding is a
worse error than asking the user to type their name more carefully.

---

## Phase 2 target resolver chain

When Phase 2 ships, the chain will extend to:

| # | Layer | Confidence |
|---|-------|-----------|
| 1 | Exact linked employee_id (from button callback) | 100 |
| 2 | Exact `@username` | 100 |
| 3 | Exact full name | 100 |
| 4 | Exact alias / abbreviation (editable table) | 92 |
| 5 | Exact first name | 100 |
| 6 | Prefix on first name | 78 |
| 7 | Trigram fuzzy (DB query, threshold 0.55) | 60–90 |
| 8 | Levenshtein fuzzy (in-memory fallback) | 60–85 |
| 9 | History boost (+10 to any layer result for creator's frequently used assignees) | modifier |
| 10 | No-match fallback | 0 |

Behavior bands will remain the same (< 75 → NotFound, 75–94 → Suggested/Ambiguous, ≥ 95 → Unique).

---

## Workload context at assignment (Phase 2 target)

When any candidate is shown to the user (Suggested or Ambiguous outcome), the candidate
button will include a passive workload hint:

```
👤 Иван Иванов   8 актив · 2 просроч
```

This is display-only. It does not block the assignment. The user remains the decision maker.

---

## No-match behavior

When all layers yield nothing (NotFound):

**In quick create / voice create:** Task is created unassigned. User sees the card with
delivery state "без исполнителя" and a CTA to assign later.

**In guided create:** User remains on the Assignee step with a message explaining
no match was found. They can try a different name, use the team browse list, or skip.

**In reassignment:** Same as guided create — no silent change, explicit retry.

---

## Abbreviation directory (Phase 2 target)

A new `employee_aliases` table stores short forms linked to employees:

```
(id, employee_id, alias, created_by_user_id, created_at)
```

- Seeded with common Russian diminutives ("Ваня" → Иван, "Маша" → Мария, ...).
- Manager/Admin can add/remove via `/admin` → Aliases.
- Aliases are matched at layer 4 of the Phase 2 chain (confidence 92).
- Alias match is unique-per-employee; if two employees share the same alias it
  produces an Ambiguous outcome, not a silent wrong assignment.
