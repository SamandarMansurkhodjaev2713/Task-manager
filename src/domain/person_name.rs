//! Canonical personal-name value object.
//!
//! Centralises *all* validation, normalisation and display rules for the
//! pair (first_name, last_name) that the onboarding flow collects.  Any
//! code that parses or formats a human name MUST go through [`PersonName`]
//! so we have a single source of truth.
//!
//! Rationale for a dedicated type (rather than `(String, String)`):
//!
//! - Invariants are enforced once, at construction time.  Consumers never
//!   have to re-validate, re-trim or re-case.
//! - Display conventions (`"Иван Иванов"` vs `"Иванов Иван"`) live with the
//!   data, not duplicated in UI formatters.
//! - `trigrams()` is intended for the assignee-suggestion index
//!   (phase 5) — keeping it on the value object guarantees the indexer and
//!   the query side agree on tokenisation.

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::domain::errors::{AppError, AppResult};

/// Maximum length (in characters) of a single name part.  Telegram first/last
/// names historically max out at 64, and our SQLite `TEXT` columns have no
/// hard limit — 64 is a sensible upper bound for display and storage.
pub const MAX_PERSON_NAME_PART_LENGTH: usize = 64;

/// Minimum length (in characters) of a single name part.  Empty is rejected;
/// the shortest real first names in the directory ("Ян", "Ли", "Ив") fit into
/// two characters, so `1` is deliberately permissive and we filter pathological
/// single-space inputs in normalisation.
pub const MIN_PERSON_NAME_PART_LENGTH: usize = 1;

/// A validated (first_name, last_name) pair.
///
/// All fields are owned `String`s — this is a small value type that is
/// expected to be cloned a few times per request, not a hot-loop structure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonName {
    first: String,
    last: String,
}

impl PersonName {
    /// Parses the pair (first, last), applying the shared normalisation
    /// rules.  Returns a validation error with a precise code if either
    /// part is empty, too long, or contains disallowed characters.
    pub fn parse(first_raw: &str, last_raw: &str) -> AppResult<Self> {
        let first = normalize_part(first_raw);
        let last = normalize_part(last_raw);
        validate_part("first_name", &first)?;
        validate_part("last_name", &last)?;
        Ok(Self { first, last })
    }

    /// Parses only the first name part; used during the `AwaitFirstName`
    /// onboarding step where the last name is not yet known.
    pub fn parse_first_only(first_raw: &str) -> AppResult<String> {
        let first = normalize_part(first_raw);
        validate_part("first_name", &first)?;
        Ok(first)
    }

    /// Parses only the last name part, assuming `first` has already been
    /// accepted.  Keeps the validation symmetric across FSM steps.
    pub fn parse_last_with_first(first: &str, last_raw: &str) -> AppResult<Self> {
        let last = normalize_part(last_raw);
        validate_part("last_name", &last)?;
        Ok(Self {
            first: first.to_owned(),
            last,
        })
    }

    /// "Иван Иванов" — the conversational form used when addressing the user.
    pub fn display(&self) -> String {
        format!("{} {}", self.first, self.last)
    }

    /// "Иванов Иван" — the directory form used in admin and audit views.
    pub fn display_reversed(&self) -> String {
        format!("{} {}", self.last, self.first)
    }

    pub fn first(&self) -> &str {
        &self.first
    }

    pub fn last(&self) -> &str {
        &self.last
    }

    /// Returns the trigram set used by the assignee-suggestion index.
    ///
    /// We lower-case, strip diacritics we don't care about, and emit 3-character
    /// sliding windows over `first + " " + last`.  For names shorter than three
    /// characters we emit the name itself (padded with the space) so that rare
    /// two-letter names still produce a stable key.
    pub fn trigrams(&self) -> Vec<String> {
        let canonical = canonical_for_trigrams(&format!("{} {}", self.first, self.last));
        let chars: Vec<char> = canonical.chars().collect();
        if chars.len() < 3 {
            return vec![canonical];
        }
        chars
            .windows(3)
            .map(|window| window.iter().collect::<String>())
            .collect()
    }
}

fn normalize_part(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn validate_part(field: &'static str, normalized: &str) -> AppResult<()> {
    let char_count = normalized.chars().count();
    if char_count < MIN_PERSON_NAME_PART_LENGTH {
        return Err(AppError::schema_validation(
            "PERSON_NAME_EMPTY",
            format!("{field} must not be empty"),
            json!({ "field": field }),
        ));
    }
    if char_count > MAX_PERSON_NAME_PART_LENGTH {
        return Err(AppError::schema_validation(
            "PERSON_NAME_TOO_LONG",
            format!("{field} exceeds the supported length"),
            json!({ "field": field, "limit": MAX_PERSON_NAME_PART_LENGTH }),
        ));
    }
    if !normalized.chars().all(is_allowed_name_character) {
        return Err(AppError::schema_validation(
            "PERSON_NAME_INVALID_CHARACTERS",
            format!("{field} contains characters that are not allowed"),
            json!({ "field": field }),
        ));
    }
    Ok(())
}

fn is_allowed_name_character(character: char) -> bool {
    character.is_alphabetic()
        || character.is_numeric()
        || character == ' '
        || character == '-'
        || character == '\''
}

fn canonical_for_trigrams(value: &str) -> String {
    value
        .to_lowercase()
        .replace('ё', "е")
        .chars()
        .filter(|character| character.is_alphanumeric() || character.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::{PersonName, MAX_PERSON_NAME_PART_LENGTH};

    #[test]
    fn given_valid_raw_parts_when_parse_then_returns_normalized_name() {
        let parsed = PersonName::parse("  Иван  ", "Иванов").expect("parse should succeed");

        assert_eq!(parsed.first(), "Иван");
        assert_eq!(parsed.last(), "Иванов");
        assert_eq!(parsed.display(), "Иван Иванов");
        assert_eq!(parsed.display_reversed(), "Иванов Иван");
    }

    #[test]
    fn given_empty_first_when_parse_then_returns_validation_error() {
        let error = PersonName::parse("", "Иванов").expect_err("parse should reject empty first");

        assert_eq!(error.code(), "PERSON_NAME_EMPTY");
    }

    #[test]
    fn given_name_exceeding_limit_when_parse_then_returns_validation_error() {
        let long = "и".repeat(MAX_PERSON_NAME_PART_LENGTH + 1);

        let error =
            PersonName::parse(&long, "Иванов").expect_err("parse should reject overlong part");

        assert_eq!(error.code(), "PERSON_NAME_TOO_LONG");
    }

    #[test]
    fn given_symbols_when_parse_then_rejects_disallowed_characters() {
        let error = PersonName::parse("Ив@н", "Иванов").expect_err("parse should reject symbols");

        assert_eq!(error.code(), "PERSON_NAME_INVALID_CHARACTERS");
    }

    #[test]
    fn given_parsed_name_when_trigrams_then_lowercases_and_replaces_yo() {
        let parsed = PersonName::parse("Алёша", "Пушкин").expect("parse should succeed");

        let trigrams = parsed.trigrams();

        assert!(trigrams.iter().any(|trigram| trigram == "але"));
        assert!(trigrams.iter().all(|trigram| !trigram.contains('ё')));
    }
}
