//! Core traits defining the LLM abstraction boundary.

use async_trait::async_trait;
use serde_json::Value;

use crate::error::Result;

#[async_trait]
/// LLM client providing both text completion and structured JSON extraction.
pub trait LlmClient: Send + Sync {
    /// Returns a plain-text completion for the supplied prompts.
    async fn complete(&self, system_prompt: &str, user_prompt: &str) -> Result<String>;

    /// Returns a JSON value that conforms to the provided extraction schema.
    async fn extract(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        json_schema: &Value,
    ) -> Result<Value>;
}
