use super::*;
use crate::error::{ReadyError, Result};
use crate::llm::traits::LlmClient;
use crate::tools::models::ToolCall;
use async_trait::async_trait;
use serde_json::Value;
use serde_json::json;
use std::sync::{Arc, Mutex};

struct MockLlm {
    complete_response: String,
    extract_response: Value,
    complete_calls: Arc<Mutex<Vec<(String, String)>>>,
    extract_calls: Arc<Mutex<Vec<(String, String, Value)>>>,
}

#[async_trait]
impl LlmClient for MockLlm {
    async fn complete(&self, system_prompt: &str, user_prompt: &str) -> Result<String> {
        self.complete_calls
            .lock()
            .expect("complete calls mutex poisoned")
            .push((system_prompt.to_string(), user_prompt.to_string()));
        Ok(self.complete_response.clone())
    }

    async fn extract(
        &self,
        system_prompt: &str,
        user_prompt: &str,
        json_schema: &Value,
    ) -> Result<Value> {
        self.extract_calls
            .lock()
            .expect("extract calls mutex poisoned")
            .push((
                system_prompt.to_string(),
                user_prompt.to_string(),
                json_schema.clone(),
            ));
        Ok(self.extract_response.clone())
    }
}

fn module() -> BuiltinToolsModule {
    let llm = Arc::new(MockLlm {
        complete_response: String::new(),
        extract_response: Value::Null,
        complete_calls: Arc::new(Mutex::new(Vec::new())),
        extract_calls: Arc::new(Mutex::new(Vec::new())),
    });
    BuiltinToolsModule::new(llm)
}

fn find_tool(module: &BuiltinToolsModule, tool_id: &str) -> ToolDescription {
    module
        .tools()
        .iter()
        .find(|tool| tool.id == tool_id)
        .cloned()
        .expect("tool should exist")
}

#[test]
fn returns_three_tools() {
    assert_eq!(module().tools().len(), 3);
}

#[test]
fn delegate_to_llm_signature() {
    let tool = find_tool(&module(), "delegate_to_large_language_model");
    assert_eq!(
        tool.arguments
            .iter()
            .map(|arg| arg.name.as_str())
            .collect::<Vec<_>>(),
        vec!["system_prompt", "user_prompt"]
    );
    assert_eq!(
        tool.arguments
            .iter()
            .map(|arg| arg.type_name.as_str())
            .collect::<Vec<&str>>(),
        vec!["str", "str"]
    );
    assert_eq!(tool.returns.type_name.as_deref(), Some("str"));
}

#[test]
fn extract_from_plaintext_signature() {
    let tool = find_tool(&module(), "extract_from_plaintext");
    assert_eq!(
        tool.arguments
            .iter()
            .map(|arg| arg.name.as_str())
            .collect::<Vec<_>>(),
        vec!["system_prompt", "plaintext", "json_schema"]
    );
    assert_eq!(
        tool.arguments
            .iter()
            .map(|arg| arg.type_name.as_str())
            .collect::<Vec<&str>>(),
        vec!["str", "str", "dict"]
    );
    assert_eq!(tool.returns.type_name.as_deref(), Some("dict"));
}

#[test]
fn sort_list_signature() {
    let tool = find_tool(&module(), "sort_list");
    assert_eq!(
        tool.arguments
            .iter()
            .map(|arg| arg.name.as_str())
            .collect::<Vec<_>>(),
        vec!["items", "key", "reverse"]
    );
    assert_eq!(
        tool.arguments
            .iter()
            .map(|arg| arg.type_name.as_str())
            .collect::<Vec<&str>>(),
        vec!["list[dict]", "str", "bool"]
    );
    assert_eq!(tool.returns.type_name.as_deref(), Some("list[dict]"));
}

#[tokio::test]
async fn delegate_uses_injected_callable() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let llm = Arc::new(MockLlm {
        complete_response: "rewritten output".to_string(),
        extract_response: Value::Null,
        complete_calls: calls.clone(),
        extract_calls: Arc::new(Mutex::new(Vec::new())),
    });
    let module = BuiltinToolsModule::new(llm);

    let call = ToolCall {
        tool_id: "delegate_to_large_language_model".to_string(),
        args: vec![json!("You are helpful"), json!("Rewrite this")],
        continuation: None,
    };
    let result = module
        .execute(&call)
        .await
        .expect("delegate tool should execute");

    assert_eq!(result, ToolResult::Success(json!("rewritten output")));
    assert_eq!(
        *calls.lock().expect("complete calls mutex poisoned"),
        vec![("You are helpful".to_string(), "Rewrite this".to_string())]
    );
}

#[tokio::test]
async fn extract_uses_injected_callable() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let llm = Arc::new(MockLlm {
        complete_response: String::new(),
        extract_response: json!({"title": "Daily Standup", "participants": 3}),
        complete_calls: Arc::new(Mutex::new(Vec::new())),
        extract_calls: calls.clone(),
    });
    let module = BuiltinToolsModule::new(llm);

    let call = ToolCall {
        tool_id: "extract_from_plaintext".to_string(),
        args: vec![
            json!(""),
            json!("Title: Daily Standup"),
            json!({"type": "object"}),
        ],
        continuation: None,
    };
    let result = module
        .execute(&call)
        .await
        .expect("extract tool should execute");

    assert_eq!(
        result,
        ToolResult::Success(json!({"title": "Daily Standup", "participants": 3}))
    );
    let calls = calls.lock().expect("extract calls mutex poisoned");
    assert_eq!(calls[0].1, "Title: Daily Standup");
    assert_eq!(calls[0].2, json!({"type": "object"}));
}

#[tokio::test]
async fn sorts_dicts_by_string_key_ascending() {
    let call = ToolCall {
        tool_id: "sort_list".to_string(),
        args: vec![
            json!([{"name": "charlie"}, {"name": "alice"}, {"name": "bob"}]),
            json!("name"),
            json!(false),
        ],
        continuation: None,
    };
    let result = module()
        .execute(&call)
        .await
        .expect("sort tool should execute");

    assert_eq!(
        result,
        ToolResult::Success(json!([
            {"name": "alice"},
            {"name": "bob"},
            {"name": "charlie"}
        ]))
    );
}

#[tokio::test]
async fn sorts_dicts_by_string_key_descending() {
    let call = ToolCall {
        tool_id: "sort_list".to_string(),
        args: vec![
            json!([{"name": "charlie"}, {"name": "alice"}, {"name": "bob"}]),
            json!("name"),
            json!(true),
        ],
        continuation: None,
    };
    let result = module()
        .execute(&call)
        .await
        .expect("sort tool should execute");

    assert_eq!(
        result,
        ToolResult::Success(json!([
            {"name": "charlie"},
            {"name": "bob"},
            {"name": "alice"}
        ]))
    );
}

#[tokio::test]
async fn sorts_dicts_by_numeric_key() {
    let call = ToolCall {
        tool_id: "sort_list".to_string(),
        args: vec![
            json!([{"priority": 3}, {"priority": 1}, {"priority": 2}]),
            json!("priority"),
            json!(false),
        ],
        continuation: None,
    };
    let result = module()
        .execute(&call)
        .await
        .expect("sort tool should execute");

    assert_eq!(
        result,
        ToolResult::Success(json!([
            {"priority": 1},
            {"priority": 2},
            {"priority": 3}
        ]))
    );
}

#[tokio::test]
async fn unknown_tool_id_raises() {
    let call = ToolCall {
        tool_id: "nonexistent_tool".to_string(),
        args: vec![],
        continuation: None,
    };
    let error = module()
        .execute(&call)
        .await
        .expect_err("unknown tool should error");

    assert!(matches!(error, ReadyError::ToolNotFound(tool_id) if tool_id == "nonexistent_tool"));
}
