//! Transcript preprocessing helpers (P1-voice-pipeline).
//!
//! ### Why this lives in the domain layer
//!
//! The transcript we receive from a speech-to-text provider is an
//! untrusted, PII-rich input that feeds the AI task generator,
//! deterministic parser, and UI surfaces (preview, confirmation, card).
//! Every consumer needs the same normalized, bounded text; putting the
//! policy anywhere but `domain/` would force every layer to re-implement
//! token budgeting and whitespace scrubbing.
//!
//! ### Contract
//!
//! * Collapses runs of whitespace (Whisper inserts `\n` between chunks).
//! * Strips leading/trailing control characters that confuse inline-mode
//!   Telegram rendering.
//! * Enforces [`MAX_TRANSCRIPT_LENGTH`] — clips at the last sentence /
//!   whitespace boundary so we never split a word.
//! * Rejects empty/whitespace-only input with a stable error code so the
//!   dispatcher can render the "empty transcript" UX.
//!
//! ### What this is NOT
//!
//! * We do not attempt to "fix" punctuation or speech-to-text errors —
//!   that is the AI layer's job (prompt hardening in §7.1) and the user
//!   always sees both the raw transcript and the AI reinterpretation.
//! * We do not strip profanity — the AI refusal rule handles abusive
//!   input at the semantic level.

use serde::{Deserialize, Serialize};

use crate::domain::errors::{AppError, AppResult};
use crate::shared::constants::limits::MAX_TRANSCRIPT_LENGTH;

/// Normalized transcript ready to feed the AI / deterministic parser.
///
/// The `truncated` flag propagates to the UI so the preview message can
/// reassure the user that we did not silently lose context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NormalizedTranscript {
    pub text: String,
    pub original_length: usize,
    pub truncated: bool,
}

impl NormalizedTranscript {
    /// Normalise a raw STT response.  Returns
    /// `AppError::Validation("TRANSCRIPTION_EMPTY")` if the input is
    /// empty once whitespace is stripped — callers render a dedicated
    /// UX for that case (see `VOICE_EMPTY_TRANSCRIPT_MESSAGE`).
    pub fn from_raw(raw: &str) -> AppResult<Self> {
        let normalised = normalise_whitespace(raw);
        if normalised.is_empty() {
            return Err(AppError::business_rule(
                "TRANSCRIPTION_EMPTY",
                "Voice message could not be transcribed into text",
                serde_json::json!({}),
            ));
        }

        let original_length = normalised.chars().count();
        let (text, truncated) = clip_to_budget(&normalised, MAX_TRANSCRIPT_LENGTH);

        Ok(Self {
            text,
            original_length,
            truncated,
        })
    }
}

/// Collapse runs of whitespace (incl. newlines and tabs) into a single
/// space and trim both ends.  Deliberately preserves inner punctuation.
fn normalise_whitespace(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut prev_space = true; // treat leading whitespace as already-consumed
    for ch in raw.chars() {
        if ch.is_control() && ch != '\n' && ch != '\r' {
            continue;
        }
        if ch.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
            continue;
        }
        out.push(ch);
        prev_space = false;
    }
    out.trim().to_owned()
}

/// Clip the string to `max_chars` characters at the nearest sentence /
/// whitespace boundary so we never split a word in half.  Returns the
/// clipped string and a `truncated` flag.
fn clip_to_budget(text: &str, max_chars: usize) -> (String, bool) {
    if text.chars().count() <= max_chars {
        return (text.to_owned(), false);
    }

    // Collect the first `max_chars` characters as byte-offsets so we can
    // slice at boundaries in O(N).
    let mut byte_end = 0usize;
    for (idx, (byte_idx, _)) in text.char_indices().enumerate() {
        if idx >= max_chars {
            break;
        }
        byte_end = byte_idx + text[byte_idx..].chars().next().unwrap().len_utf8();
    }

    // Walk backwards to the last sentence or whitespace boundary — at most
    // 160 chars back so we never throw away useful content.
    let window = &text[..byte_end];
    let backoff_limit = window.len().saturating_sub(160);
    let breakpoint = window
        .char_indices()
        .rev()
        .find(|(idx, ch)| *idx >= backoff_limit && (*ch == '.' || *ch == '!' || *ch == '?'))
        .map(|(idx, ch)| idx + ch.len_utf8())
        .or_else(|| {
            window
                .char_indices()
                .rev()
                .find(|(idx, ch)| *idx >= backoff_limit && ch.is_whitespace())
                .map(|(idx, _)| idx)
        })
        .unwrap_or(byte_end);

    let mut clipped = text[..breakpoint].trim_end().to_owned();
    // Show the user that something was cut so they can decide to re-
    // record; this is surfaced in the UI through `truncated`, not the
    // text itself, but we add a terse marker for the AI prompt.
    clipped.push('…');
    (clipped, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_whitespace_heavy_input_when_normalised_then_collapsed_and_trimmed() {
        let t = NormalizedTranscript::from_raw("  Подготовь  отчёт\n\nк пятнице   ").unwrap();
        assert_eq!(t.text, "Подготовь отчёт к пятнице");
        assert!(!t.truncated);
    }

    #[test]
    fn given_empty_input_when_normalised_then_returns_stable_error_code() {
        let err = NormalizedTranscript::from_raw("   \n  ").unwrap_err();
        assert_eq!(err.code(), "TRANSCRIPTION_EMPTY");
    }

    #[test]
    fn given_oversized_input_when_normalised_then_clips_and_flags_truncated() {
        let long = "Подготовить отчёт. ".repeat(1_000); // ≈ 19 000 chars
        let t = NormalizedTranscript::from_raw(&long).unwrap();
        assert!(t.truncated);
        assert!(
            t.text.chars().count() <= MAX_TRANSCRIPT_LENGTH + 1,
            "clipped length: {}",
            t.text.chars().count()
        );
        assert!(t.text.ends_with('…'));
    }

    #[test]
    fn given_mid_word_boundary_when_clipping_then_backoff_to_whitespace() {
        let long = format!("{}ABCDEFG", "slово ".repeat(MAX_TRANSCRIPT_LENGTH));
        let t = NormalizedTranscript::from_raw(&long).unwrap();
        assert!(t.truncated);
        assert!(!t.text.contains("ABCDEFG"));
    }
}
