//! OpenAI-compatible HTTP client implementation.

use std::env;

use async_trait::async_trait;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde_json::{Value, json};
use tracing::{debug, trace, warn};

use crate::error::{ReadyError, Result};
use crate::llm::traits::LlmClient;

const DEFAULT_MODEL: &str = "gpt-4o";
const DEFAULT_API_BASE: &str = "https://api.openai.com/v1";
const DEFAULT_TEMPERATURE: f32 = 0.0;

#[derive(Debug, Clone)]
/// HTTP client for OpenAI-compatible APIs with configurable model, base URL, and API key.
pub struct OpenAiClient {
    http: reqwest::Client,
    model: String,
    api_base: String,
    api_key: Option<String>,
}

impl OpenAiClient {
    /// Constructs a client from explicit overrides, then environment variables, then defaults.
    pub fn new(model: Option<String>, api_base: Option<String>, api_key: Option<String>) -> Self {
        let model = model
            .or_else(|| env::var("READY_MODEL").ok())
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let api_base = api_base
            .or_else(|| env::var("READY_API_BASE").ok())
            .unwrap_or_else(|| DEFAULT_API_BASE.to_string());
        let api_key = api_key.or_else(|| env::var("OPENAI_API_KEY").ok());

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Some(api_key) = &api_key
            && let Ok(value) = HeaderValue::from_str(&format!("Bearer {api_key}"))
        {
            headers.insert(AUTHORIZATION, value);
        }

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            http,
            model,
            api_base: api_base.trim_end_matches('/').to_string(),
            api_key,
        }
    }

    fn chat_completions_url(&self) -> String {
        format!("{}/chat/completions", self.api_base)
    }

    async fn post_chat_completion(&self, payload: Value) -> Result<String> {
        debug!(
            model = %self.model,
            api_base = %self.api_base,
            payload = %payload,
            "sending LLM chat completion request"
        );

        let response = self
            .http
            .post(self.chat_completions_url())
            .json(&payload)
            .send()
            .await?;
        let status = response.status();
        let body: Value = response.json().await?;

        debug!(status = %status, body = %body, "received LLM chat completion response");

        if !status.is_success() {
            warn!(status = %status, body = %body, "LLM chat completion request failed");
            return Err(ReadyError::Llm(format!(
                "OpenAI-compatible API returned {status}: {}",
                body
            )));
        }

        extract_message_content(&body)
    }

    async fn complete_with_json_schema(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        json_schema: &Value,
    ) -> Result<Value> {
        trace!(
            system_prompt = system_prompt,
            user_prompt = user_prompt,
            json_schema = %json_schema,
            "requesting structured LLM extraction"
        );

        // OpenAI strict mode requires additionalProperties: false and every property key
        // listed in `required` at every object level.
        let mut schema = json_schema.clone();
        enforce_strict_schema(&mut schema);

        let raw = self
            .post_chat_completion(json!({
                "model": &self.model,
                "temperature": DEFAULT_TEMPERATURE,
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": user_prompt}
                ],
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {
                        "name": "extraction",
                        "strict": true,
                        "schema": schema
                    }
                }
            }))
            .await?;

        trace!(raw_response = raw, "received structured LLM extraction response");

        serde_json::from_str::<Value>(&raw).map_err(|error| {
            ReadyError::Llm(format!(
                "Structured-output response was not valid JSON: {error}; raw response: {raw:?}"
            ))
        })
    }

    async fn complete_with_fallback(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        json_schema: &Value,
    ) -> Result<Value> {
        let schema_text = serde_json::to_string_pretty(json_schema)?;
        let fallback_system_prompt = format!(
            "{system_prompt}\n\nRespond with a JSON object only — no prose, no markdown fences. The JSON must conform to this schema:\n{schema_text}"
        );

        debug!(
            system_prompt = fallback_system_prompt,
            user_prompt = user_prompt,
            "falling back to plain-text structured extraction"
        );

        let raw = self.complete(&fallback_system_prompt, user_prompt).await?;
        let stripped = strip_markdown_fences(&raw);

        trace!(raw_response = raw, stripped_response = stripped, "received fallback structured extraction response");

        serde_json::from_str::<Value>(&stripped).map_err(|error| {
            ReadyError::Llm(format!(
                "Fallback structured extraction could not parse JSON: {error}; raw response: {raw:?}"
            ))
        })
    }
}

/// Creates a default client configuration from environment variables and built-in defaults.
impl Default for OpenAiClient {
    fn default() -> Self {
        Self::new(None, None, None)
    }
}

#[async_trait]
/// Implements LlmClient for OpenAI-compatible endpoints with text completion and structured extraction.
impl LlmClient for OpenAiClient {
    async fn complete(&self, system_prompt: &str, user_prompt: &str) -> Result<String> {
        let _ = &self.api_key;
        trace!(system_prompt = system_prompt, user_prompt = user_prompt, "requesting text completion from LLM");
        self.post_chat_completion(json!({
            "model": &self.model,
            "temperature": DEFAULT_TEMPERATURE,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ]
        }))
        .await
    }

    async fn extract(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        json_schema: &Value,
    ) -> Result<Value> {
        match self
            .complete_with_json_schema(system_prompt, user_prompt, json_schema)
            .await
        {
            Ok(value) => Ok(value),
            Err(error) => {
                warn!(error = %error, "structured extraction failed; retrying with fallback mode");
                self.complete_with_fallback(system_prompt, user_prompt, json_schema)
                    .await
            }
        }
    }
}

/// Recursively enforces OpenAI strict-mode schema requirements:
/// - `additionalProperties: false` on every object
/// - `required` listing every key in `properties`
fn enforce_strict_schema(schema: &mut Value) {
    let Some(obj) = schema.as_object_mut() else { return };

    if let Some(props) = obj.get("properties").cloned() {
        if let Some(keys) = props.as_object().map(|m| m.keys().cloned().collect::<Vec<_>>()) {
            obj.insert("required".to_string(), Value::Array(keys.into_iter().map(Value::String).collect()));
        }
        obj.insert("additionalProperties".to_string(), Value::Bool(false));

        // Recurse into each property value.
        if let Some(props_mut) = obj.get_mut("properties").and_then(Value::as_object_mut) {
            for val in props_mut.values_mut() {
                enforce_strict_schema(val);
            }
        }
    }
}

fn extract_message_content(body: &Value) -> Result<String> {
    let Some(content) = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
    else {
        return Err(ReadyError::Llm(format!(
            "Missing choices[0].message.content in response: {body}"
        )));
    };

    match content {
        Value::Null => Ok(String::new()),
        Value::String(text) => Ok(text.clone()),
        Value::Array(parts) => {
            let mut text = String::new();
            for part in parts {
                if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                    text.push_str(part_text);
                }
            }
            Ok(text)
        }
        other => Ok(other.to_string()),
    }
}

/// Strips surrounding markdown code fences such as ```python and ``` from LLM output.
pub fn strip_markdown_fences(raw: &str) -> String {
    let stripped = raw.trim();
    if !stripped.starts_with("```") {
        return stripped.to_string();
    }

    let lines = stripped.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return String::new();
    }

    let start = 1;
    let end = if lines.last().is_some_and(|line| line.trim() == "```") {
        lines.len().saturating_sub(1)
    } else {
        lines.len()
    };

    lines[start..end].join("\n").trim().to_string()
}
