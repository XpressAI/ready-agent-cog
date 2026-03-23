//! Built-in tools, including LLM delegation, plaintext extraction, and list sorting.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::{ReadyError, Result};
use crate::llm::traits::LlmClient;
use crate::tools::models::{
    ToolArgumentDescription, ToolCall, ToolDescription, ToolResult, ToolReturnDescription,
};
use crate::tools::traits::ToolsModule;

const DEFAULT_EXTRACTION_PROMPT: &str = "Extract structured information from the provided text according to the supplied JSON Schema. Return only the extracted data; do not add commentary.";

/// Provides the built-in tools backed by a shared LLM client.
pub struct BuiltinToolsModule {
    llm: Arc<dyn LlmClient>,
    extraction_system_prompt: String,
    descriptions: Vec<ToolDescription>,
}

impl BuiltinToolsModule {
    /// Constructs the built-in module with the default extraction system prompt.
    pub fn new(llm: Arc<dyn LlmClient>) -> Self {
        Self::with_extraction_system_prompt(llm, DEFAULT_EXTRACTION_PROMPT)
    }

    /// Constructs the module with a caller-supplied extraction system prompt.
    pub fn with_extraction_system_prompt(
        llm: Arc<dyn LlmClient>,
        extraction_system_prompt: impl Into<String>,
    ) -> Self {
        Self {
            llm,
            extraction_system_prompt: extraction_system_prompt.into(),
            descriptions: vec![
                Self::delegate_description(),
                Self::extract_description(),
                Self::sort_description(),
            ],
        }
    }

    fn delegate_description() -> ToolDescription {
        ToolDescription {
            id: "delegate_to_large_language_model".to_string(),
            description: "Delegates a task to a large language model. Use this when you need rephrasing, summarization, or other free-form language generation.".to_string(),
            arguments: vec![
                ToolArgumentDescription {
                    name: "system_prompt".to_string(),
                    description: "Instructions for what you want the LLM to do.".to_string(),
                    type_name: "str".to_string(),
                    default: None,
                },
                ToolArgumentDescription {
                    name: "user_prompt".to_string(),
                    description: "The task/input data for the LLM.".to_string(),
                    type_name: "str".to_string(),
                    default: None,
                },
            ],
            returns: ToolReturnDescription {
                name: Some("output".to_string()),
                description: "LLM response text.".to_string(),
                type_name: Some("str".to_string()),
                fields: Vec::new(),
            },
        }
    }

    fn extract_description() -> ToolDescription {
        ToolDescription {
            id: "extract_from_plaintext".to_string(),
            description: "Extracts any information from a string. Use this whenever you need to pull, parse, or read anything out of a string value — for example extracting a name, date, number, URL, status, or any other field; filtering or selecting items from text; converting free-form text into a typed value; or answering a question about the contents of a string. Pass the string as plain_text and a JSON Schema describing what you want back as schema.".to_string(),
            arguments: vec![
                ToolArgumentDescription {
                    name: "system_prompt".to_string(),
                    description: "Plain text that contains information to extract.".to_string(),
                    type_name: "str".to_string(),
                    default: None,
                },
                ToolArgumentDescription {
                    name: "plaintext".to_string(),
                    description: "Plain text that contains information to extract.".to_string(),
                    type_name: "str".to_string(),
                    default: None,
                },
                ToolArgumentDescription {
                    name: "json_schema".to_string(),
                    description: "JSON Schema definition that specifies the expected output structure.".to_string(),
                    type_name: "dict".to_string(),
                    default: None,
                },
            ],
            returns: ToolReturnDescription {
                name: Some("output".to_string()),
                description: "Extracted structured data.".to_string(),
                type_name: Some("dict".to_string()),
                fields: Vec::new(),
            },
        }
    }

    fn sort_description() -> ToolDescription {
        ToolDescription {
            id: "sort_list".to_string(),
            description: "Sorts a list of dictionaries or objects by the specified key or attribute and returns a new list.".to_string(),
            arguments: vec![
                ToolArgumentDescription {
                    name: "items".to_string(),
                    description: "List of dictionaries or objects to sort.".to_string(),
                    type_name: "list[dict]".to_string(),
                    default: None,
                },
                ToolArgumentDescription {
                    name: "key".to_string(),
                    description: "Dictionary key or object attribute name to sort by.".to_string(),
                    type_name: "str".to_string(),
                    default: None,
                },
                ToolArgumentDescription {
                    name: "reverse".to_string(),
                    description: "Whether to sort in descending order.".to_string(),
                    type_name: "bool".to_string(),
                    default: Some("False".to_string()),
                },
            ],
            returns: ToolReturnDescription {
                name: Some("output".to_string()),
                description: "A new list sorted by the requested key.".to_string(),
                type_name: Some("list[dict]".to_string()),
                fields: Vec::new(),
            },
        }
    }

    fn value_as_str(value: &Value, tool_id: &str, arg_name: &str) -> Result<String> {
        value
            .as_str()
            .map(ToOwned::to_owned)
            .ok_or_else(|| ReadyError::Tool {
                tool_id: tool_id.to_string(),
                message: format!("Argument '{}' must be a string", arg_name),
            })
    }
}

/// Dispatches execution for the built-in tools.
#[async_trait]
impl ToolsModule for BuiltinToolsModule {
    fn tools(&self) -> &[ToolDescription] {
        &self.descriptions
    }

    async fn execute(&self, call: &ToolCall) -> Result<ToolResult> {
        let tool_id = call.tool_id.as_str();
        let args = &call.args;
        match tool_id {
            "delegate_to_large_language_model" => {
                if args.len() < 2 {
                    return Err(ReadyError::Tool {
                        tool_id: tool_id.to_string(),
                        message: "Expected arguments: system_prompt, user_prompt".to_string(),
                    });
                }
                let system_prompt = Self::value_as_str(&args[0], tool_id, "system_prompt")?;
                let user_prompt = Self::value_as_str(&args[1], tool_id, "user_prompt")?;
                let output = self.llm.complete(&system_prompt, &user_prompt).await?;
                Ok(ToolResult::Success(Value::String(output)))
            }
            "extract_from_plaintext" => {
                if args.len() < 3 {
                    return Err(ReadyError::Tool {
                        tool_id: tool_id.to_string(),
                        message: "Expected arguments: system_prompt, plaintext, json_schema"
                            .to_string(),
                    });
                }
                let system_prompt = Self::value_as_str(&args[0], tool_id, "system_prompt")?;
                let user_prompt = Self::value_as_str(&args[1], tool_id, "plaintext")?;
                let schema = &args[2];
                let output = self
                    .llm
                    .extract(
                        if system_prompt.is_empty() {
                            &self.extraction_system_prompt
                        } else {
                            &system_prompt
                        },
                        &user_prompt,
                        schema,
                    )
                    .await?;
                Ok(ToolResult::Success(output))
            }
            "sort_list" => {
                if args.len() < 3 {
                    return Err(ReadyError::Tool {
                        tool_id: tool_id.to_string(),
                        message: "Expected arguments: items, key, reverse".to_string(),
                    });
                }

                let items = args[0]
                    .as_array()
                    .cloned()
                    .ok_or_else(|| ReadyError::Tool {
                        tool_id: tool_id.to_string(),
                        message: "Argument 'items' must be a list".to_string(),
                    })?;
                let key = Self::value_as_str(&args[1], tool_id, "key")?;
                let reverse = args[2].as_bool().ok_or_else(|| ReadyError::Tool {
                    tool_id: tool_id.to_string(),
                    message: "Argument 'reverse' must be a bool".to_string(),
                })?;

                let mut items = items;
                items.sort_by(|left, right| {
                    let left_value = sortable_value(left, &key);
                    let right_value = sortable_value(right, &key);
                    compare_json_values(left_value, right_value)
                });
                if reverse {
                    items.reverse();
                }
                Ok(ToolResult::Success(Value::Array(items)))
            }
            _ => Err(ReadyError::ToolNotFound(tool_id.to_string())),
        }
    }
}

fn sortable_value<'a>(item: &'a Value, key: &str) -> &'a Value {
    match item {
        Value::Object(map) => map.get(key).unwrap_or(&Value::Null),
        _ => &Value::Null,
    }
}

fn compare_json_values(left: &Value, right: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    match (left, right) {
        (Value::String(a), Value::String(b)) => a.cmp(b),
        (Value::Number(a), Value::Number(b)) => a
            .as_f64()
            .partial_cmp(&b.as_f64())
            .unwrap_or(Ordering::Equal),
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        (Value::Null, Value::Null) => Ordering::Equal,
        _ => left.to_string().cmp(&right.to_string()),
    }
}

#[cfg(test)]
#[path = "builtin_tests.rs"]
mod tests;
