use uuid::Uuid;

use crate::shared::constants::limits::PUBLIC_TASK_CODE_WIDTH;

const PUBLIC_TASK_CODE_PREFIX: &str = "T";
const PUBLIC_TASK_CODE_SEPARATOR: &str = "-";
const PUBLIC_TASK_CODE_PLACEHOLDER_NUMBER: &str = "NEW";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskReference {
    PublicId(i64),
    Uid(Uuid),
}

pub fn format_public_task_code(task_id: i64) -> String {
    format!(
        "{PUBLIC_TASK_CODE_PREFIX}{PUBLIC_TASK_CODE_SEPARATOR}{task_id:0width$}",
        width = PUBLIC_TASK_CODE_WIDTH
    )
}

pub fn format_public_task_code_or_placeholder(task_id: Option<i64>) -> String {
    match task_id {
        Some(value) if value > 0 => format_public_task_code(value),
        _ => format!(
            "{PUBLIC_TASK_CODE_PREFIX}{PUBLIC_TASK_CODE_SEPARATOR}{PUBLIC_TASK_CODE_PLACEHOLDER_NUMBER}"
        ),
    }
}

pub fn parse_task_reference(value: &str) -> Option<TaskReference> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    parse_public_task_code(trimmed)
        .map(TaskReference::PublicId)
        .or_else(|| Uuid::parse_str(trimmed).ok().map(TaskReference::Uid))
}

pub fn parse_public_task_code(value: &str) -> Option<i64> {
    let normalized = value.trim().to_ascii_uppercase();
    let numeric_part = normalized
        .strip_prefix(PUBLIC_TASK_CODE_PREFIX)?
        .strip_prefix(PUBLIC_TASK_CODE_SEPARATOR)?;
    if numeric_part.is_empty() {
        return None;
    }

    numeric_part
        .parse::<i64>()
        .ok()
        .filter(|task_id| *task_id > 0)
}

#[cfg(test)]
mod tests {
    use super::{
        format_public_task_code, parse_public_task_code, parse_task_reference, TaskReference,
    };

    #[test]
    fn format_public_task_code_zero_pads_identifier() {
        assert_eq!(format_public_task_code(42), "T-0042");
    }

    #[test]
    fn parse_public_task_code_accepts_prefixed_value() {
        assert_eq!(parse_public_task_code("t-0042"), Some(42));
    }

    #[test]
    fn parse_task_reference_understands_public_code() {
        assert_eq!(
            parse_task_reference("T-0042"),
            Some(TaskReference::PublicId(42))
        );
    }
}
