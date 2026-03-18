//! Data models for tool descriptions, arguments, return types, and execution results.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// Describes a single named field, including its type and optional nested fields.
pub struct FieldDescription {
    pub name: String,
    pub description: String,
    pub type_name: String,
    #[serde(default)]
    pub fields: Vec<FieldDescription>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// Describes a tool return value, including its type and any structured fields.
pub struct ToolReturnDescription {
    pub name: Option<String>,
    pub description: String,
    pub type_name: Option<String>,
    #[serde(default)]
    pub fields: Vec<FieldDescription>,
}

impl ToolReturnDescription {
    /// Renders this structured return type as a Python `@dataclass` stub when applicable.
    pub fn to_class_stub(&self) -> Option<String> {
        match (&self.type_name, self.fields.is_empty()) {
            (Some(type_name), false) => {
                Some(render_class_stub(element_type_name(type_name), &self.fields))
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// Describes a single positional argument accepted by a tool.
pub struct ToolArgumentDescription {
    pub name: String,
    pub description: String,
    pub type_name: String,
    pub default: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// Describes a tool fully, including identifiers, inputs, and output metadata.
pub struct ToolDescription {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub arguments: Vec<ToolArgumentDescription>,
    pub returns: ToolReturnDescription,
}

impl ToolDescription {
    /// Renders this tool description as a Python function stub for prompt construction.
    pub fn to_python_stub(&self) -> String {
        let params = self
            .arguments
            .iter()
            .map(|arg| match &arg.default {
                Some(default) => format!("{}: {} = {}", arg.name, arg.type_name, default),
                None => format!("{}: {}", arg.name, arg.type_name),
            })
            .collect::<Vec<_>>()
            .join(", ");

        let return_type = self
            .returns
            .type_name
            .clone()
            .unwrap_or_else(|| "Any".to_string());

        let mut lines = vec![format!("def {}({}) -> {}:", self.id, params, return_type)];
        if !self.description.is_empty() {
            lines.push(format!("    \"\"\"{}\"\"\"", self.description));
        }
        lines.push("    ...".to_string());
        lines.join("\n")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// Carries the reason for suspension together with opaque continuation state.
pub struct ToolSuspension {
    pub reason: String,
    pub continuation_state: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// Represents either a completed tool result or a suspended execution.
pub enum ToolResult {
    Success(Value),
    Suspended(ToolSuspension),
}

/// Carries the suspension state for a tool that was previously suspended.
#[derive(Debug, Clone)]
pub struct Continuation {
    pub state: Value,
    pub resume_value: Option<Value>,
}

/// A structured tool invocation request passed to [`crate::tools::traits::ToolsModule::execute`].
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub tool_id: String,
    pub args: Vec<Value>,
    pub continuation: Option<Continuation>,
}

/// Generates Python stub text for tool descriptions, deduplicating emitted class stubs.
pub fn generate_prompt_stubs(descriptions: &[ToolDescription]) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut class_stubs = Vec::new();

    for description in descriptions {
        collect_class_stubs(
            &description.returns.fields,
            description.returns.type_name.as_deref(),
            &mut seen,
            &mut class_stubs,
        );
    }

    let function_stubs = descriptions
        .iter()
        .map(ToolDescription::to_python_stub)
        .collect::<Vec<_>>();

    class_stubs
        .into_iter()
        .chain(function_stubs)
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Renders a named structured type as a Python `@dataclass`-style class stub.
pub fn render_class_stub(type_name: &str, fields: &[FieldDescription]) -> String {
    let mut lines = vec![format!("class {}:", type_name)];
    for field in fields {
        let mut line = format!("    {}: {}", field.name, field.type_name);
        if !field.description.is_empty() {
            line.push_str(&format!("  # {}", field.description));
        }
        lines.push(line);
    }
    lines.join("\n")
}

fn element_type_name(type_name: &str) -> &str {
    type_name
        .strip_prefix("list[")
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(type_name)
}

fn collect_class_stubs(
    fields: &[FieldDescription],
    type_name: Option<&str>,
    seen: &mut std::collections::HashSet<String>,
    out: &mut Vec<String>,
) {
    for field in fields {
        if !field.fields.is_empty() {
            collect_class_stubs(
                &field.fields,
                Some(element_type_name(&field.type_name)),
                seen,
                out,
            );
        }
    }

    if let Some(type_name) = type_name
        && !fields.is_empty()
        && seen.insert(element_type_name(type_name).to_string())
    {
        out.push(render_class_stub(element_type_name(type_name), fields));
    }
}

#[cfg(test)]
#[path = "models_tests.rs"]
mod tests;
