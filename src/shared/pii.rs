//! PII redaction helpers used by the structured logger and audit recorders.
//!
//! Principle: **never** log raw user-generated text, names, phone numbers,
//! usernames, or other identifying tokens.  Instead, log a stable, hashed
//! pseudonym so that a single incident can be correlated without
//! reconstructing the content.
//!
//! The redactor is process-scoped — it carries a per-process salt so that
//! hashes are not reversible across deployments.  A nil-salt variant exists
//! for deterministic test assertions.

use std::sync::OnceLock;

use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct PIIRedactor {
    salt: Vec<u8>,
}

impl PIIRedactor {
    pub fn new(salt: impl Into<Vec<u8>>) -> Self {
        Self { salt: salt.into() }
    }

    /// Redactor used by unit tests — empty salt gives deterministic hashes.
    pub fn nil() -> Self {
        Self { salt: Vec::new() }
    }

    /// Returns the short, salted hash suitable for log lines.  Example:
    /// `"pii:4a8f3d"`.
    pub fn redact(&self, input: &str) -> String {
        if input.is_empty() {
            return String::from("pii:empty");
        }
        let mut hasher = Sha256::new();
        hasher.update(&self.salt);
        hasher.update(input.as_bytes());
        let digest = hasher.finalize();
        format!("pii:{:x}{:x}{:x}", digest[0], digest[1], digest[2])
    }

    /// Convenience: redact an optional field without allocating "None" as a
    /// string (important for tracing-sized log buffers).
    pub fn redact_opt(&self, input: Option<&str>) -> String {
        match input {
            Some(v) => self.redact(v),
            None => String::from("pii:none"),
        }
    }
}

/// Global redactor instance, initialised once from configuration.  Use this
/// from modules that don't already take a redactor as a dependency.
static GLOBAL_REDACTOR: OnceLock<PIIRedactor> = OnceLock::new();

pub fn install_global_redactor(redactor: PIIRedactor) {
    let _ = GLOBAL_REDACTOR.set(redactor);
}

pub fn global_redactor() -> &'static PIIRedactor {
    GLOBAL_REDACTOR.get_or_init(PIIRedactor::nil)
}

#[cfg(test)]
mod tests {
    use super::PIIRedactor;

    #[test]
    fn given_same_input_when_redacted_then_same_output() {
        let r = PIIRedactor::nil();

        assert_eq!(r.redact("Иван Иванов"), r.redact("Иван Иванов"));
    }

    #[test]
    fn given_different_salts_when_redacted_then_different_output() {
        let a = PIIRedactor::new(b"salt-a".to_vec());
        let b = PIIRedactor::new(b"salt-b".to_vec());

        assert_ne!(a.redact("user@example.com"), b.redact("user@example.com"));
    }

    #[test]
    fn given_empty_input_when_redacted_then_marker_returned() {
        let r = PIIRedactor::nil();

        assert_eq!(r.redact(""), "pii:empty");
    }
}
