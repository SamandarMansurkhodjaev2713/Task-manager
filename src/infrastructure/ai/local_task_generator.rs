use async_trait::async_trait;

use crate::application::ports::services::{GeneratedTask, TaskGenerator};
use crate::domain::employee::Employee;
use crate::domain::errors::AppResult;
use crate::domain::message::ParsedTaskRequest;
use crate::domain::task::StructuredTaskDraft;
use crate::shared::constants::limits::{
    MAX_TASK_EXPECTED_RESULT_LENGTH, MAX_TASK_STEP_LENGTH, MAX_TASK_TITLE_LENGTH,
};

const LOCAL_TASK_GENERATOR_MODEL: &str = "local-rule-based-generator";

pub struct LocalTaskGenerator;

#[async_trait]
impl TaskGenerator for LocalTaskGenerator {
    async fn generate_task(
        &self,
        parsed_request: &ParsedTaskRequest,
        assignee: Option<&Employee>,
    ) -> AppResult<GeneratedTask> {
        let assignee_display = assignee
            .map(|employee| employee.full_name.as_str())
            .or(parsed_request.assignee_name.as_deref());
        let title = build_title(&parsed_request.task_description);
        let steps = build_steps(&parsed_request.task_description);
        let expected_result = build_expected_result(parsed_request, assignee_display);
        let acceptance_criteria = build_acceptance_criteria(parsed_request);
        let structured_task = StructuredTaskDraft {
            title,
            expected_result,
            steps,
            acceptance_criteria,
            deadline_iso: parsed_request
                .deadline
                .map(|date| date.format("%Y-%m-%d").to_string()),
            refused: false,
            refusal_reason: None,
        };
        structured_task.validate_business_rules()?;

        Ok(GeneratedTask {
            model_name: LOCAL_TASK_GENERATOR_MODEL.to_owned(),
            raw_response: serde_json::to_string_pretty(&structured_task)
                .unwrap_or_else(|_| "{}".to_owned()),
            structured_task,
        })
    }
}

fn build_title(description: &str) -> String {
    let first_sentence = description
        .split(['.', '!', '?'])
        .map(str::trim)
        .find(|value| !value.is_empty())
        .unwrap_or(description);

    truncate_chars(first_sentence, MAX_TASK_TITLE_LENGTH)
}

fn build_steps(description: &str) -> Vec<String> {
    let items = description
        .split([',', ';', '\n'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_chars(value, MAX_TASK_STEP_LENGTH))
        .collect::<Vec<_>>();

    if items.is_empty() {
        return vec![truncate_chars(description.trim(), MAX_TASK_STEP_LENGTH)];
    }

    items
}

fn build_expected_result(
    parsed_request: &ParsedTaskRequest,
    assignee_display: Option<&str>,
) -> String {
    let mut parts = vec!["Задача выполнена и готова к проверке.".to_owned()];
    if let Some(assignee_display) = assignee_display {
        parts.push(format!("Исполнитель: {assignee_display}."));
    }
    if let Some(deadline) = parsed_request.deadline {
        parts.push(format!("Срок: {}.", deadline.format("%d.%m.%Y")));
    }

    truncate_chars(&parts.join(" "), MAX_TASK_EXPECTED_RESULT_LENGTH)
}

fn build_acceptance_criteria(parsed_request: &ParsedTaskRequest) -> Vec<String> {
    let mut criteria = vec!["Результат можно проверить по факту выполнения.".to_owned()];
    if let Some(deadline) = parsed_request.deadline {
        criteria.push(format!("Срок соблюдён: {}.", deadline.format("%d.%m.%Y")));
    }
    criteria
}

fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect::<String>()
}
