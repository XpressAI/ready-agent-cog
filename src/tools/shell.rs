//! Shell command tools that execute templated commands and parse their results.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::process::Command;

use crate::error::{ReadyError, Result};
use crate::tools::models::{
    ToolArgumentDescription, ToolCall, ToolDescription, ToolResult, ToolReturnDescription,
};
use crate::tools::traits::ToolsModule;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
/// Defines how stdout should be parsed into a tool result value.
pub enum OutputParsing {
    Raw,
    Json,
    Int,
    Float,
    Bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// Defines a shell-backed tool, including its template, schema, and runtime settings.
pub struct ShellToolEntry {
    pub description: String,
    pub template: Vec<String>,
    #[serde(default)]
    pub arguments: Vec<ToolArgumentDescription>,
    pub returns: ToolReturnDescription,
    pub active: bool,
    pub output_parsing: OutputParsing,
    pub output_schema: Option<Value>,
}

/// Loads and saves shell tool definitions from JSON files.
pub struct ShellToolStore;

impl ShellToolStore {
    /// Loads shell tool entries from a JSON file, returning an empty store if the file is missing.
    pub fn load(path: impl AsRef<Path>) -> Result<HashMap<String, ShellToolEntry>> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let content = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&content)?)
    }

    /// Saves shell tool entries to a JSON file using pretty-printed JSON.
    pub fn save(path: impl AsRef<Path>, entries: &HashMap<String, ShellToolEntry>) -> Result<()> {
        let content = serde_json::to_string_pretty(entries)?;
        fs::write(path, content)?;
        Ok(())
    }
}

/// Wraps shell tool entries so they can be exposed and executed as a tool module.
pub struct ShellToolsModule {
    entries: HashMap<String, ShellToolEntry>,
    descriptions: Vec<ToolDescription>,
    runner: std::sync::Arc<dyn ShellCommandRunner>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ShellCommandOutput {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) trait ShellCommandRunner: Send + Sync {
    fn run<'a>(
        &'a self,
        command: Vec<String>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ShellCommandOutput>> + Send + 'a>>;
}

struct TokioCommandRunner;

impl ShellCommandRunner for TokioCommandRunner {
    fn run<'a>(
        &'a self,
        command: Vec<String>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ShellCommandOutput>> + Send + 'a>> {
        Box::pin(async move {
            let mut process = Command::new(command.first().ok_or_else(|| ReadyError::Tool {
                tool_id: "<unknown>".to_string(),
                message: "Shell tool template must contain at least one command part".to_string(),
            })?);

            if command.len() > 1 {
                process.args(&command[1..]);
            }

            let output = process.output().await?;
            Ok(ShellCommandOutput {
                success: output.status.success(),
                exit_code: output.status.code(),
                stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            })
        })
    }
}

impl ShellToolsModule {
    /// Constructs a shell tools module from the provided entry map.
    pub fn new(entries: HashMap<String, ShellToolEntry>) -> Self {
        Self::with_runner(entries, std::sync::Arc::new(TokioCommandRunner))
    }

    pub(crate) fn with_runner(
        entries: HashMap<String, ShellToolEntry>,
        runner: std::sync::Arc<dyn ShellCommandRunner>,
    ) -> Self {
        let descriptions = build_descriptions(&entries);
        Self {
            entries,
            descriptions,
            runner,
        }
    }

    pub(crate) fn render_command(
        entry: &ShellToolEntry,
        args: &[Value],
        tool_id: &str,
    ) -> Result<Vec<String>> {
        let mut bound = HashMap::new();
        for (description, value) in entry.arguments.iter().zip(args.iter()) {
            bound.insert(description.name.clone(), value_to_template_string(value));
        }

        entry
            .template
            .iter()
            .map(|part| render_template_part(part, &bound, tool_id))
            .collect()
    }

    pub(crate) fn parse_command_output(
        entry: &ShellToolEntry,
        output: ShellCommandOutput,
        tool_id: &str,
    ) -> Result<Value> {
        if !matches!(entry.output_parsing, OutputParsing::Raw) && !output.success {
            return Err(ReadyError::Tool {
                tool_id: tool_id.to_string(),
                message: format!(
                    "Command failed (exit {:?}): {}",
                    output.exit_code, output.stderr
                ),
            });
        }

        parse_output(
            &entry.output_parsing,
            &output.stdout,
            &output.stderr,
            tool_id,
        )
    }
}

pub(crate) fn build_descriptions(
    entries: &HashMap<String, ShellToolEntry>,
) -> Vec<ToolDescription> {
    entries
        .iter()
        .filter(|(_, entry)| entry.active)
        .map(|(tool_id, entry)| ToolDescription {
            id: tool_id.clone(),
            description: entry.description.clone(),
            arguments: entry.arguments.clone(),
            returns: entry.returns.clone(),
        })
        .collect()
}

/// Executes a rendered shell command and parses its output into a tool result.
impl ToolsModule for ShellToolsModule {
    fn tools(&self) -> &[ToolDescription] {
        &self.descriptions
    }

    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let tool_id = call.tool_id.as_str();
            let args = &call.args;
            let entry = self
                .entries
                .get(tool_id)
                .ok_or_else(|| ReadyError::ToolNotFound(tool_id.to_string()))?;

            let rendered = Self::render_command(entry, args, tool_id)?;
            let output = self.runner.run(rendered).await?;
            let parsed = Self::parse_command_output(entry, output, tool_id)?;

            Ok(ToolResult::Success(parsed))
        })
    }
}

pub(crate) fn parse_output(
    parsing: &OutputParsing,
    stdout: &str,
    stderr: &str,
    tool_id: &str,
) -> Result<Value> {
    match parsing {
        OutputParsing::Raw => Ok(Value::String(format!("{}{}", stdout, stderr))),
        OutputParsing::Json => Ok(serde_json::from_str::<Value>(stdout)?),
        OutputParsing::Int => Ok(Value::from(stdout.trim().parse::<i64>().map_err(
            |err| ReadyError::Tool {
                tool_id: tool_id.to_string(),
                message: format!("Failed to parse integer output: {err}"),
            },
        )?)),
        OutputParsing::Float => Ok(Value::from(stdout.trim().parse::<f64>().map_err(
            |err| ReadyError::Tool {
                tool_id: tool_id.to_string(),
                message: format!("Failed to parse float output: {err}"),
            },
        )?)),
        OutputParsing::Bool => Ok(Value::Bool(matches!(
            stdout.trim().to_ascii_lowercase().as_str(),
            "true" | "1" | "yes"
        ))),
    }
}

fn value_to_template_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => value.to_string(),
    }
}

pub(crate) fn render_template_part(
    part: &str,
    bound: &HashMap<String, String>,
    tool_id: &str,
) -> Result<String> {
    let mut rendered = String::new();
    let mut chars = part.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut key = String::new();
            for next in chars.by_ref() {
                if next == '}' {
                    break;
                }
                key.push(next);
            }

            let value = bound.get(&key).ok_or_else(|| ReadyError::Tool {
                tool_id: tool_id.to_string(),
                message: format!("Template placeholder '{}' not satisfied", key),
            })?;
            rendered.push_str(value);
        } else {
            rendered.push(ch);
        }
    }

    Ok(rendered)
}

#[cfg(test)]
#[path = "shell_tests.rs"]
mod tests;
