use std::time::Duration;

use async_trait::async_trait;
use reqwest::multipart::{Form, Part};
use reqwest::{Client, Response};
use secrecy::ExposeSecret;
use serde::Deserialize;

use crate::application::ports::services::SpeechToTextService;
use crate::config::{OpenAiConfig, TelegramConfig};
use crate::domain::errors::{AppError, AppResult};
use crate::domain::message::VoiceAttachment;
use crate::infrastructure::http::circuit_breaker::CircuitBreaker;
use crate::infrastructure::http::retry::retry_with_backoff;
use crate::shared::constants::limits::{MAX_AUDIO_DURATION_SECONDS, MAX_AUDIO_FILE_SIZE_BYTES};
use crate::shared::constants::timeouts::OPENAI_TRANSCRIPTION_TIMEOUT_SECONDS;

const TELEGRAM_API_BASE_URL: &str = "https://api.telegram.org";
const OPENAI_TRANSCRIPTION_URL: &str = "https://api.openai.com/v1/audio/transcriptions";

pub struct OpenAiTranscriptionClient {
    client: Client,
    telegram_config: TelegramConfig,
    openai_config: OpenAiConfig,
    circuit_breaker: CircuitBreaker,
}

impl OpenAiTranscriptionClient {
    pub fn new(telegram_config: TelegramConfig, openai_config: OpenAiConfig) -> AppResult<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(OPENAI_TRANSCRIPTION_TIMEOUT_SECONDS))
            .build()
            .map_err(|error| {
                AppError::internal(
                    "HTTP_CLIENT_BUILD_FAILED",
                    "Failed to create OpenAI transcription client",
                    serde_json::json!({ "error": error.to_string() }),
                )
            })?;

        Ok(Self {
            client,
            telegram_config,
            openai_config,
            circuit_breaker: CircuitBreaker::new(),
        })
    }

    async fn download_voice(&self, voice: &VoiceAttachment) -> AppResult<Vec<u8>> {
        validate_voice_metadata(voice)?;
        let get_file_url = format!(
            "{}/bot{}/getFile",
            TELEGRAM_API_BASE_URL,
            self.telegram_config.bot_token.expose_secret()
        );
        let file_response = self
            .client
            .get(get_file_url)
            .query(&[("file_id", voice.file_id.as_str())])
            .send()
            .await
            .map_err(|error| {
                AppError::network(
                    "TELEGRAM_FILE_LOOKUP_FAILED",
                    "Failed to request voice file metadata from Telegram",
                    serde_json::json!({ "error": error.to_string() }),
                )
            })?;
        let file_response = ensure_success_status(
            file_response,
            "TELEGRAM_FILE_LOOKUP_HTTP_STATUS",
            "Telegram file lookup returned a non-success status",
        )?;
        let file_payload = file_response
            .json::<TelegramFileLookupResponse>()
            .await
            .map_err(|error| {
                AppError::network(
                    "TELEGRAM_FILE_LOOKUP_INVALID",
                    "Telegram file lookup response is invalid",
                    serde_json::json!({ "error": error.to_string() }),
                )
            })?;
        if !file_payload.ok {
            return Err(AppError::network(
                "TELEGRAM_FILE_LOOKUP_REJECTED",
                "Telegram rejected the voice file lookup request",
                serde_json::json!({ "description": file_payload.description }),
            ));
        }
        let file_path = file_payload
            .result
            .and_then(|result| result.file_path)
            .ok_or_else(|| {
                AppError::network(
                    "TELEGRAM_FILE_PATH_MISSING",
                    "Telegram did not return a downloadable file path",
                    serde_json::json!({ "file_id": voice.file_id }),
                )
            })?;

        let download_url = format!(
            "{}/file/bot{}/{}",
            TELEGRAM_API_BASE_URL,
            self.telegram_config.bot_token.expose_secret(),
            file_path
        );
        let bytes = self
            .client
            .get(download_url)
            .send()
            .await
            .map_err(|error| {
                AppError::network(
                    "TELEGRAM_FILE_DOWNLOAD_FAILED",
                    "Failed to download voice payload from Telegram",
                    serde_json::json!({ "error": error.to_string() }),
                )
            })?;
        let bytes = ensure_success_status(
            bytes,
            "TELEGRAM_FILE_DOWNLOAD_HTTP_STATUS",
            "Telegram file download returned a non-success status",
        )?;
        Ok(bytes
            .bytes()
            .await
            .map_err(|error| {
                AppError::network(
                    "TELEGRAM_FILE_BYTES_INVALID",
                    "Failed to read downloaded voice bytes",
                    serde_json::json!({ "error": error.to_string() }),
                )
            })?
            .to_vec())
    }

    async fn transcribe_bytes(&self, audio_bytes: &[u8]) -> AppResult<String> {
        self.circuit_breaker
            .ensure_closed("openai_transcription")
            .await?;
        let form = Form::new()
            .text("model", self.openai_config.transcription_model.clone())
            .text("language", "ru".to_owned())
            .part(
                "file",
                Part::bytes(audio_bytes.to_vec())
                    .file_name("voice.ogg")
                    .mime_str("audio/ogg")
                    .map_err(|error| {
                        AppError::internal(
                            "OPENAI_MULTIPART_INVALID",
                            "Failed to build multipart payload for transcription",
                            serde_json::json!({ "error": error.to_string() }),
                        )
                    })?,
            );

        let response = self
            .client
            .post(OPENAI_TRANSCRIPTION_URL)
            .bearer_auth(self.openai_config.api_key.expose_secret())
            .multipart(form)
            .send()
            .await
            .map_err(|error| {
                AppError::network(
                    "OPENAI_TRANSCRIPTION_FAILED",
                    "Failed to request transcription from OpenAI",
                    serde_json::json!({ "error": error.to_string() }),
                )
            })?;

        if !response.status().is_success() {
            self.circuit_breaker.record_failure().await;
            return Err(AppError::network(
                "OPENAI_TRANSCRIPTION_HTTP_STATUS",
                "OpenAI transcription API returned a non-success status",
                serde_json::json!({ "status": response.status().as_u16() }),
            ));
        }

        let payload = response
            .json::<OpenAiTranscriptionResponse>()
            .await
            .map_err(|error| {
                AppError::network(
                    "OPENAI_TRANSCRIPTION_RESPONSE_INVALID",
                    "OpenAI transcription response is invalid",
                    serde_json::json!({ "error": error.to_string() }),
                )
            })?;
        if payload.text.trim().is_empty() {
            return Err(AppError::business_rule(
                "TRANSCRIPTION_EMPTY",
                "Voice message could not be transcribed into text",
                serde_json::json!({}),
            ));
        }

        self.circuit_breaker.record_success().await;
        Ok(payload.text.trim().to_owned())
    }
}

#[async_trait]
impl SpeechToTextService for OpenAiTranscriptionClient {
    async fn transcribe(&self, voice: &VoiceAttachment) -> AppResult<String> {
        // Both download and transcription are retried independently:
        // download can fail on transient network errors before we have any bytes,
        // transcription can fail after a successful download.
        let audio_bytes = retry_with_backoff(|| self.download_voice(voice)).await?;
        retry_with_backoff(|| self.transcribe_bytes(&audio_bytes)).await
    }
}

fn ensure_success_status(
    response: Response,
    code: &'static str,
    message: &'static str,
) -> AppResult<Response> {
    if response.status().is_success() {
        return Ok(response);
    }

    Err(AppError::network(
        code,
        message,
        serde_json::json!({ "status": response.status().as_u16() }),
    ))
}

fn validate_voice_metadata(voice: &VoiceAttachment) -> AppResult<()> {
    if voice.duration_seconds > MAX_AUDIO_DURATION_SECONDS {
        return Err(AppError::business_rule(
            "VOICE_TOO_LONG",
            "Voice message duration exceeds the supported limit",
            serde_json::json!({ "max_seconds": MAX_AUDIO_DURATION_SECONDS }),
        ));
    }

    if let Some(file_size_bytes) = voice.file_size_bytes {
        if file_size_bytes > MAX_AUDIO_FILE_SIZE_BYTES {
            return Err(AppError::business_rule(
                "VOICE_TOO_LARGE",
                "Voice message file size exceeds the supported limit",
                serde_json::json!({ "max_bytes": MAX_AUDIO_FILE_SIZE_BYTES }),
            ));
        }
    }

    Ok(())
}

#[derive(Debug, Deserialize)]
struct TelegramFileLookupResponse {
    ok: bool,
    #[serde(default)]
    result: Option<TelegramFileResult>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramFileResult {
    #[serde(default)]
    file_path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiTranscriptionResponse {
    text: String,
}
