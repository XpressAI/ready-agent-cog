use super::runtime::InMemoryToolRegistry;
use crate::error::{ReadyError, Result};
use crate::tools::models::{ToolCall, ToolDescription, ToolResult};
use crate::tools::traits::ToolsModule;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

struct StubToolsModule {
    tools: Vec<ToolDescription>,
    results: HashMap<String, Value>,
    calls: Arc<Mutex<Vec<(String, Vec<Value>)>>>,
}

#[async_trait]
impl ToolsModule for StubToolsModule {
    fn tools(&self) -> &[ToolDescription] {
        &self.tools
    }

    async fn execute(&self, call: &ToolCall) -> Result<ToolResult> {
        self.calls
            .lock()
            .expect("calls mutex poisoned")
            .push((call.tool_id.clone(), call.args.clone()));
        let result = match self.results.get(&call.tool_id) {
            Some(value) => Ok(ToolResult::Success(value.clone())),
            None => Err(ReadyError::ToolNotFound(call.tool_id.clone())),
        };
        result
    }
}

fn make_stub_module(
    tool_ids: &[&str],
    results: &[(&str, Value)],
) -> (Box<dyn ToolsModule>, Arc<Mutex<Vec<(String, Vec<Value>)>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let module = StubToolsModule {
        tools: tool_ids
            .iter()
            .map(|tool_id| description(tool_id))
            .collect(),
        results: results
            .iter()
            .map(|(tool_id, value)| ((*tool_id).to_string(), value.clone()))
            .collect(),
        calls: calls.clone(),
    };
    (Box::new(module), calls)
}

fn description(tool_id: &str) -> ToolDescription {
    ToolDescription {
        id: tool_id.to_string(),
        description: String::new(),
        arguments: Vec::new(),
        returns: crate::tools::models::ToolReturnDescription {
            name: None,
            description: String::new(),
            type_name: None,
            fields: Vec::new(),
        },
    }
}

#[test]
fn empty_registry_returns_no_tools() {
    let registry = InMemoryToolRegistry::new();
    assert!(registry.tools().is_empty());
}

#[test]
fn single_module_lists_its_tools() {
    let mut registry = InMemoryToolRegistry::new();
    let (module, _calls) = make_stub_module(
        &["tool_a1", "tool_a2"],
        &[
            ("tool_a1", json!("result_a1")),
            ("tool_a2", json!("result_a2")),
        ],
    );
    registry.register_module(module).unwrap();

    let ids = registry
        .tools()
        .iter()
        .map(|tool| tool.id.clone())
        .collect::<std::collections::HashSet<_>>();

    assert_eq!(
        ids,
        std::collections::HashSet::from(["tool_a1".to_string(), "tool_a2".to_string()])
    );
}

#[tokio::test]
async fn routes_to_owning_module() {
    let mut registry = InMemoryToolRegistry::new();
    let (module_a, _calls_a) = make_stub_module(&["tool_a1"], &[("tool_a1", json!("result_a1"))]);
    let (module_b, calls_b) = make_stub_module(&["tool_b1"], &[("tool_b1", json!("result_b1"))]);
    registry.register_module(module_a).unwrap();
    registry.register_module(module_b).unwrap();

    let call = ToolCall {
        tool_id: "tool_b1".to_string(),
        args: vec![json!("arg1")],
        continuation: None,
    };
    let result = registry.execute(&call).await.expect("tool should execute");

    assert_eq!(result, ToolResult::Success(json!("result_b1")));
    assert_eq!(
        *calls_b.lock().expect("calls mutex poisoned"),
        vec![("tool_b1".to_string(), vec![json!("arg1")])]
    );
}

#[tokio::test]
async fn unknown_tool_raises() {
    let registry = InMemoryToolRegistry::new();

    let call = ToolCall {
        tool_id: "nonexistent".to_string(),
        args: vec![],
        continuation: None,
    };
    let error = registry
        .execute(&call)
        .await
        .expect_err("unknown tool should error");

    assert!(matches!(error, ReadyError::ToolNotFound(tool_id) if tool_id == "nonexistent"));
}

#[test]
fn duplicate_tool_registration_returns_error() {
    let mut registry = InMemoryToolRegistry::new();

    let (first, _calls_first) = make_stub_module(&["tool_x"], &[("tool_x", json!("first"))]);
    let (second, _calls_second) = make_stub_module(&["tool_x"], &[("tool_x", json!("second"))]);

    registry.register_module(first).unwrap();
    let error = registry
        .register_module(second)
        .expect_err("duplicate tool registration should fail");

    let listed = registry.tools();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "tool_x");
    assert!(matches!(error, ReadyError::DuplicateTool(tool_id) if tool_id == "tool_x"));
}

#[test]
fn has_tool_reflects_registered_ids() {
    let mut registry = InMemoryToolRegistry::new();
    let (module, _calls) = make_stub_module(&["tool_a", "tool_b"], &[]);
    registry.register_module(module).unwrap();

    assert!(registry.has_tool("tool_a"));
    assert!(registry.has_tool("tool_b"));
    assert!(!registry.has_tool("tool_c"));
}
