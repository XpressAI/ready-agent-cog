use super::*;
use crate::error::{ReadyError, Result};
use crate::plan::AbstractPlan;
use crate::plan::{BranchKind, ConditionalBranch, Expression, Step};
use crate::test_helpers::to_literal;
use crate::tools::models::{Continuation, ToolCall};
use crate::tools::process::load_plans_from_directory;
use crate::tools::traits::ToolsModule;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

struct MockModule {
    descriptions: Vec<ToolDescription>,
    handler: Arc<dyn Fn(&str, Vec<Value>) -> Result<ToolResult> + Send + Sync>,
}

impl ToolsModule for MockModule {
    fn tools(&self) -> &[ToolDescription] {
        &self.descriptions
    }

    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send + 'a>> {
        let result = (self.handler)(call.tool_id.as_str(), call.args.clone());
        Box::pin(async move { result })
    }
}

fn tool_description(tool_id: &str, arguments: &[&str]) -> ToolDescription {
    ToolDescription {
        id: tool_id.to_string(),
        description: String::new(),
        arguments: arguments
            .iter()
            .map(|name| ToolArgumentDescription {
                name: (*name).to_string(),
                description: String::new(),
                type_name: "str".to_string(),
                default: None,
            })
            .collect(),
        returns: ToolReturnDescription {
            name: None,
            description: String::new(),
            type_name: None,
            fields: Vec::new(),
        },
    }
}

fn plan(name: &str, steps: Vec<Step>) -> AbstractPlan {
    AbstractPlan {
        name: name.to_string(),
        description: String::new(),
        steps,
        code: String::new(),
    }
}

fn tool_step(tool_id: &str, arguments: Vec<Expression>, output_variable: Option<&str>) -> Step {
    Step::ToolStep {
        tool_id: tool_id.to_string(),
        arguments,
        output_variable: output_variable.map(str::to_string),
    }
}

fn user_step(prompt: &str, output_variable: &str) -> Step {
    Step::UserInteractionStep {
        prompt: prompt.to_string(),
        output_variable: Some(output_variable.to_string()),
    }
}

fn assign_step(name: &str, value: Value) -> Step {
    Step::AssignStep {
        target: name.to_string(),
        value: Expression::Literal {
            value: to_literal(value),
        },
    }
}

fn access(name: &str) -> Expression {
    Expression::AccessPath {
        variable_name: name.to_string(),
        accessors: Vec::new(),
    }
}

fn registry_with_echo(calls: Arc<Mutex<Vec<Value>>>) -> InMemoryToolRegistry {
    let mut registry = InMemoryToolRegistry::new();
    registry
        .register_module(Box::new(MockModule {
            descriptions: vec![tool_description("echo", &["value"])],
            handler: Arc::new(move |_tool_id, args| {
                calls
                    .lock()
                    .expect("calls mutex poisoned")
                    .push(args[0].clone());
                Ok(ToolResult::Success(args[0].clone()))
            }),
        }))
        .unwrap();
    registry
}

fn registry_with_noop() -> InMemoryToolRegistry {
    let mut registry = InMemoryToolRegistry::new();
    registry
        .register_module(Box::new(MockModule {
            descriptions: vec![tool_description("noop", &[])],
            handler: Arc::new(|_, _| Ok(ToolResult::Success(Value::Null))),
        }))
        .unwrap();
    registry
}

fn temp_dir_path(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should move forward")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("ready_process_tests_{name}_{nanos}"));
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
}

#[test]
fn list_tools_returns_one_entry_per_plan_and_prefillable_inputs() {
    let registry = registry_with_noop();
    let module = ProcessToolsModule::new(
        HashMap::from([(
            "echo_plan".to_string(),
            plan(
                "echo_plan",
                vec![
                    user_step("Enter a value:", "user_value"),
                    tool_step("noop", vec![], None),
                ],
            ),
        )]),
        registry,
    )
    .expect("module should construct");

    let listed = module.tools();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "echo_plan");
    assert_eq!(listed[0].arguments.len(), 1);
    assert_eq!(listed[0].arguments[0].name, "user_value");
    assert_eq!(listed[0].arguments[0].description, "Enter a value:");
}

#[tokio::test]
async fn execute_tool_maps_positional_inputs_and_completes() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let registry = registry_with_echo(calls.clone());
    let module = ProcessToolsModule::new(
        HashMap::from([(
            "echo_plan".to_string(),
            plan(
                "echo_plan",
                vec![
                    user_step("Enter a value:", "user_value"),
                    tool_step("echo", vec![access("user_value")], Some("result")),
                ],
            ),
        )]),
        registry,
    )
    .expect("module should construct");

    let call = ToolCall {
        tool_id: "echo_plan".to_string(),
        args: vec![json!("hello")],
        continuation: None,
    };
    let result = module
        .execute(&call)
        .await
        .expect("process should complete");

    assert_eq!(result, ToolResult::Success(Value::Null));
    assert_eq!(
        *calls.lock().expect("calls mutex poisoned"),
        vec![json!("hello")]
    );
}

#[tokio::test]
async fn execute_tool_suspends_and_resumes() {
    let registry = registry_with_noop();
    let module = ProcessToolsModule::new(
        HashMap::from([(
            "interactive".to_string(),
            plan(
                "interactive",
                vec![
                    user_step("What is your name?", "name"),
                    tool_step("noop", vec![], None),
                ],
            ),
        )]),
        registry,
    )
    .expect("module should construct");

    let call = ToolCall {
        tool_id: "interactive".to_string(),
        args: vec![],
        continuation: None,
    };
    let suspended = module
        .execute(&call)
        .await
        .expect("initial run should suspend");

    let ToolResult::Suspended(suspension) = suspended else {
        panic!("expected suspension result");
    };
    assert!(suspension.reason.contains("What is your name?"));

    let resume_call = ToolCall {
        tool_id: "interactive".to_string(),
        args: vec![],
        continuation: Some(Continuation {
            state: suspension.continuation_state,
            resume_value: Some(json!("Alice")),
        }),
    };
    let resumed = module
        .execute(&resume_call)
        .await
        .expect("resume should complete");

    assert_eq!(resumed, ToolResult::Success(Value::Null));
}

#[test]
fn construction_rejects_unknown_tool_references_but_allows_sibling_plans() {
    let registry = registry_with_noop();

    let error = ProcessToolsModule::new(
        HashMap::from([(
            "broken".to_string(),
            plan("broken", vec![tool_step("missing_tool", vec![], None)]),
        )]),
        registry_with_noop(),
    )
    .err()
    .expect("unknown tool should fail validation");
    assert!(
        matches!(error, ReadyError::PlanValidation(message) if message.contains("missing_tool"))
    );

    let sibling = ProcessToolsModule::new(
        HashMap::from([
            (
                "plan_a".to_string(),
                plan("plan_a", vec![tool_step("plan_b", vec![], None)]),
            ),
            (
                "plan_b".to_string(),
                plan("plan_b", vec![tool_step("noop", vec![], None)]),
            ),
        ]),
        registry,
    )
    .expect("sibling plan references should be allowed");
    let ids = sibling
        .tools()
        .iter()
        .map(|tool| tool.id.clone())
        .collect::<Vec<_>>();
    assert!(ids.contains(&"plan_a".to_string()));
    assert!(ids.contains(&"plan_b".to_string()));
}

#[test]
fn load_plans_from_directory_reads_only_plan_json_files() {
    let directory = temp_dir_path("load_plans");
    let alpha = plan("alpha", vec![assign_step("x", json!(1))]);
    let beta = plan(
        "beta",
        vec![Step::SwitchStep {
            branches: vec![ConditionalBranch {
                kind: BranchKind::Else,
                condition: None,
                steps: vec![],
            }],
        }],
    );

    fs::write(
        directory.join("alpha_plan.json"),
        serde_json::to_string(&alpha).expect("alpha should serialize"),
    )
    .expect("alpha should write");
    fs::write(
        directory.join("beta_plan.json"),
        serde_json::to_string(&beta).expect("beta should serialize"),
    )
    .expect("beta should write");
    fs::write(directory.join("config.json"), "{}").expect("non-plan file should write");

    let loaded = load_plans_from_directory(&directory).expect("plans should load");
    assert_eq!(loaded.len(), 2);
    assert!(loaded.contains_key("alpha"));
    assert!(loaded.contains_key("beta"));

    let _ = fs::remove_dir_all(directory);
}
