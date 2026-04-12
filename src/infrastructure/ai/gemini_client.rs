use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};

use crate::application::ports::services::{GeneratedTask, TaskGenerator};
use crate::config::GeminiConfig;
use crate::domain::employee::Employee;
use crate::domain::errors::{AppError, AppResult};
use crate::domain::message::ParsedTaskRequest;
use crate::domain::task::StructuredTaskDraft;
use crate::infrastructure::http::circuit_breaker::CircuitBreaker;
use crate::infrastructure::http::retry::retry_with_backoff;
use crate::shared::constants::timeouts::GEMINI_TIMEOUT_SECONDS;

const GEMINI_API_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";
const SYSTEM_PROMPT: &str = r#"Ты — AI-ассистент в системе управления задачами.
Преобразуй входящее сообщение в строгий JSON без пояснений.
Запрещено выдумывать детали, которых нет во входе.
Верни JSON следующего вида:
{
  "title": "краткий заголовок до 100 символов",
  "expected_result": "измеримый результат",
  "steps": ["шаг 1", "шаг 2"],
  "acceptance_criteria": ["критерий 1", "критерий 2"]
}"#;

pub struct GeminiTaskGenerator {
    client: Client,
    config: GeminiConfig,
    circuit_breaker: CircuitBreaker,
}

impl GeminiTaskGenerator {
    pub fn new(config: GeminiConfig) -> AppResult<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(GEMINI_TIMEOUT_SECONDS))
            .build()
            .map_err(|error| {
                AppError::internal(
                    "HTTP_CLIENT_BUILD_FAILED",
                    "Failed to create Gemini HTTP client",
                    serde_json::json!({ "error": error.to_string() }),
                )
            })?;

        Ok(Self {
            client,
            config,
            circuit_breaker: CircuitBreaker::new(),
        })
    }

    async fn perform_request(
        &self,
        parsed_request: &ParsedTaskRequest,
        assignee: Option<&Employee>,
    ) -> AppResult<GeneratedTask> {
        self.circuit_breaker.ensure_closed("gemini").await?;

        let url = format!(
            "{}/{}:generateContent?key={}",
            GEMINI_API_BASE_URL,
            self.config.model,
            self.config.api_key.expose_secret()
        );
        let payload = GeminiGenerateRequest::from_input(parsed_request, assignee);
        let response = self
            .client
            .post(url)
            .json(&payload)
            .send()
            .await
            .map_err(|error| {
                AppError::network(
                    "GEMINI_REQUEST_FAILED",
                    "Gemini request failed",
                    serde_json::json!({ "error": error.to_string() }),
                )
            })?;

        if !response.status().is_success() {
            self.circuit_breaker.record_failure().await;
            return Err(AppError::network(
                "GEMINI_HTTP_STATUS",
                "Gemini returned a non-success status code",
                serde_json::json!({ "status": response.status().as_u16() }),
            ));
        }

        let body = response
            .json::<GeminiGenerateResponse>()
            .await
            .map_err(|error| {
                AppError::network(
                    "GEMINI_RESPONSE_INVALID",
                    "Gemini response is not valid JSON",
                    serde_json::json!({ "error": error.to_string() }),
                )
            })?;
        let raw_response = body
            .candidates
            .first()
            .and_then(|candidate| candidate.content.parts.first())
            .map(|part| part.text.clone())
            .ok_or_else(|| {
                AppError::network(
                    "GEMINI_RESPONSE_EMPTY",
                    "Gemini response did not contain task JSON",
                    serde_json::json!({}),
                )
            })?;
        let structured_task =
            serde_json::from_str::<StructuredTaskDraft>(&raw_response).map_err(|error| {
                AppError::schema_validation(
                    "GEMINI_SCHEMA_INVALID",
                    "Gemini response does not match the expected schema",
                    serde_json::json!({ "error": error.to_string(), "raw_response": raw_response }),
                )
            })?;
        structured_task.validate_business_rules()?;
        self.circuit_breaker.record_success().await;

        Ok(GeneratedTask {
            model_name: self.config.model.clone(),
            raw_response,
            structured_task,
        })
    }
}

#[async_trait]
impl TaskGenerator for GeminiTaskGenerator {
    async fn generate_task(
        &self,
        parsed_request: &ParsedTaskRequest,
        assignee: Option<&Employee>,
    ) -> AppResult<GeneratedTask> {
        retry_with_backoff(|| self.perform_request(parsed_request, assignee)).await
    }
}

#[derive(Debug, Serialize)]
struct GeminiGenerateRequest {
    #[serde(rename = "system_instruction")]
    system_instruction: GeminiInstruction,
    contents: Vec<GeminiContent>,
    #[serde(rename = "generationConfig")]
    generation_config: GeminiGenerationConfig,
}

impl GeminiGenerateRequest {
    fn from_input(parsed_request: &ParsedTaskRequest, assignee: Option<&Employee>) -> Self {
        let assignee_display = assignee
            .map(|employee| employee.full_name.as_str())
            .or(parsed_request.assignee_name.as_deref())
            .unwrap_or("Не указан");

        let user_prompt = format!(
            "Исполнитель: {assignee_display}\nОписание: {}\nСрок: {}\nТребуется JSON без пояснений.",
            parsed_request.task_description,
            parsed_request
                .deadline
                .map(|date| date.format("%d.%m.%Y").to_string())
                .unwrap_or_else(|| "Срок не указан".to_owned())
        );

        Self {
            system_instruction: GeminiInstruction::new(SYSTEM_PROMPT),
            contents: vec![GeminiContent::user(user_prompt)],
            generation_config: GeminiGenerationConfig {
                temperature: 0.2,
                response_mime_type: "application/json".to_owned(),
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct GeminiInstruction {
    parts: Vec<GeminiPart>,
}

impl GeminiInstruction {
    fn new(text: &str) -> Self {
        Self {
            parts: vec![GeminiPart {
                text: text.to_owned(),
            }],
        }
    }
}

#[derive(Debug, Serialize)]
struct GeminiContent {
    role: String,
    parts: Vec<GeminiPart>,
}

impl GeminiContent {
    fn user(text: String) -> Self {
        Self {
            role: "user".to_owned(),
            parts: vec![GeminiPart { text }],
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct GeminiPart {
    text: String,
}

#[derive(Debug, Serialize)]
struct GeminiGenerationConfig {
    temperature: f32,
    #[serde(rename = "responseMimeType")]
    response_mime_type: String,
}

#[derive(Debug, Deserialize)]
struct GeminiGenerateResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: GeminiCandidateContent,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidateContent {
    parts: Vec<GeminiPart>,
}
