//! [`DirectoryDigestProvider`] implementation backed by
//! [`EmployeeRepository`].
//!
//! ### Why a dedicated adapter (P1-ai-prompt-hardening)
//!
//! * Keeps the clean-architecture contract — `GeminiTaskGenerator`
//!   (infrastructure) only depends on an application-layer port.
//! * Centralises the size-budget for the prompt context in one place.
//!   Gemini is reasonably tolerant but we still keep the roster under
//!   [`MAX_DIGEST_BYTES`] to avoid context bloat and to keep each turn
//!   reproducible across large directories.
//! * The digest is PII-minimised: only the canonical full name and
//!   `@username` go into the prompt.  Phone numbers, email, and
//!   telegram-ids never leak to the AI.
//! * Output is deterministic (sorted by `full_name`) to make prompt
//!   caching / regression tests stable.

use std::sync::Arc;

use async_trait::async_trait;

use crate::application::ports::repositories::EmployeeRepository;
use crate::application::ports::services::DirectoryDigestProvider;
use crate::domain::errors::AppResult;

/// Soft limit on the digest size we inject into the Gemini user prompt.
/// 8 KiB is well below Gemini's 32K context window and leaves room for
/// the system prompt and the user's original text.  Lines beyond this
/// threshold are silently truncated (an explicit "… ещё N" footer makes
/// the truncation visible to the model).
const MAX_DIGEST_BYTES: usize = 8 * 1024;

pub struct EmployeeDirectoryDigest {
    employees: Arc<dyn EmployeeRepository>,
}

impl EmployeeDirectoryDigest {
    pub fn new(employees: Arc<dyn EmployeeRepository>) -> Self {
        Self { employees }
    }
}

#[async_trait]
impl DirectoryDigestProvider for EmployeeDirectoryDigest {
    async fn fetch_digest(&self) -> AppResult<String> {
        let mut employees = self.employees.list_active().await?;
        // Deterministic ordering — makes prompts reproducible and eases
        // debugging ("why did the AI pick Ivan over Igor yesterday?").
        employees.sort_by(|a, b| a.full_name.cmp(&b.full_name));
        Ok(render_digest(&employees))
    }
}

fn render_digest(employees: &[crate::domain::employee::Employee]) -> String {
    let mut out = String::new();
    let mut truncated = 0usize;
    for (idx, employee) in employees.iter().enumerate() {
        let line = match employee.telegram_username.as_deref() {
            Some(handle) if !handle.is_empty() => {
                format!(
                    "{} — @{}",
                    employee.full_name,
                    handle.trim_start_matches('@')
                )
            }
            _ => employee.full_name.clone(),
        };

        // +1 for the newline we'd append below.
        if out.len() + line.len() + 1 > MAX_DIGEST_BYTES {
            truncated = employees.len() - idx;
            break;
        }

        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&line);
    }

    if truncated > 0 {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&format!(
            "… ещё {} сотрудников не вошли в контекст",
            truncated
        ));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::employee::Employee;
    use chrono::Utc;

    fn sample(full_name: &str, username: Option<&str>) -> Employee {
        let now = Utc::now();
        Employee {
            id: None,
            full_name: full_name.to_owned(),
            telegram_username: username.map(|s| s.to_owned()),
            email: None,
            phone: None,
            department: None,
            is_active: true,
            synced_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn given_employees_with_and_without_username_when_rendering_then_lines_formatted() {
        let digest = render_digest(&[
            sample("Иван Иванов", Some("ivanov")),
            sample("Петр Петров", None),
        ]);
        assert!(digest.contains("Иван Иванов — @ivanov"));
        assert!(digest.contains("Петр Петров"));
        assert!(!digest.contains("— @ \n"));
    }

    #[test]
    fn given_too_many_employees_when_rendering_then_truncates_and_reports_remainder() {
        let big_name = "И".repeat(300);
        let employees: Vec<Employee> = (0..200)
            .map(|i| sample(&format!("{}_{}", big_name, i), None))
            .collect();
        let digest = render_digest(&employees);
        assert!(digest.len() <= MAX_DIGEST_BYTES + 128); // + footer
        assert!(digest.contains("ещё"));
    }
}
