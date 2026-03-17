use super::*;
use crate::error::ReadyError;
use crate::plan::AbstractPlan;
use crate::plan::{BranchKind, ConditionalBranch, Expression, Step};
use crate::test_helpers::{HandlerToolsModule, to_literal};
use crate::tools::models::{Continuation, ToolCall};
use crate::tools::process::ProcessPlanRunner;
use crate::tools::process::build_process_descriptions;
use crate::tools::process::load_plans_from_directory;
use crate::tools::process::map_execution_state_to_tool_result;
use crate::tools::process::seed_execution_state;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

struct StubProcessRunner {
    events: Arc<Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl ProcessPlanRunner for StubProcessRunner {
    async fn execute(
        &self,
        _registry: Arc<InMemoryToolRegistry>,
        _plan: &AbstractPlan,
        state: &mut crate::execution::state::ExecutionState,
    ) -> crate::error::Result<()> {
        self.events
            .lock()
            .expect("events mutex poisoned")
            .push("execute".to_string());
        if state.interpreter_state.variables.contains_key("user_value") {
            state.status = crate::execution::state::ExecutionStatus::Completed;
        } else {
            state.status = crate::execution::state::ExecutionStatus::Suspended;
            state.suspension_reason = Some("What is your name?".to_string());
        }
        Ok(())
    }

    async fn provide_input(
        &self,
        _registry: Arc<InMemoryToolRegistry>,
        _plan: &AbstractPlan,
        state: &mut crate::execution::state::ExecutionState,
        value: Value,
    ) -> crate::error::Result<()> {
        self.events
            .lock()
            .expect("events mutex poisoned")
            .push(format!("resume:{value}"));
        state
            .interpreter_state
            .variables
            .insert("name".to_string(), value);
        state.status = crate::execution::state::ExecutionStatus::Completed;
        state.suspension_reason = None;
        Ok(())
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
        .register_module(Box::new(HandlerToolsModule::new(
            vec![tool_description("echo", &["value"])],
            move |_tool_id, args| {
                calls
                    .lock()
                    .expect("calls mutex poisoned")
                    .push(args[0].clone());
                Ok(ToolResult::Success(args[0].clone()))
            },
        )))
        .unwrap();
    registry
}

fn registry_with_noop() -> InMemoryToolRegistry {
    let mut registry = InMemoryToolRegistry::new();
    registry
        .register_module(Box::new(HandlerToolsModule::new(
            vec![tool_description("noop", &[])],
            |_, _| Ok(ToolResult::Success(Value::Null)),
        )))
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

fn process_module_with_runner(
    plans: HashMap<String, AbstractPlan>,
    registry: InMemoryToolRegistry,
    events: Arc<Mutex<Vec<String>>>,
) -> ProcessToolsModule {
    let runner: Arc<dyn ProcessPlanRunner> = Arc::new(StubProcessRunner { events });
    ProcessToolsModule::with_runner(plans, registry, runner).expect("module should construct")
}

#[test]
fn list_tools_returns_one_entry_per_plan_and_prefillable_inputs() {
    let listed = build_process_descriptions(&HashMap::from([(
        "echo_plan".to_string(),
        plan(
            "echo_plan",
            vec![
                user_step("Enter a value:", "user_value"),
                tool_step("noop", vec![], None),
            ],
        ),
    )]));
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "echo_plan");
    assert_eq!(listed[0].arguments.len(), 1);
    assert_eq!(listed[0].arguments[0].name, "user_value");
    assert_eq!(listed[0].arguments[0].description, "Enter a value:");
}

#[tokio::test]
async fn execute_tool_maps_positional_inputs_and_completes() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let module = process_module_with_runner(
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
        registry_with_echo(Arc::new(Mutex::new(Vec::new()))),
        events.clone(),
    );

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
        *events.lock().expect("events mutex poisoned"),
        vec!["execute".to_string()]
    );
}

#[tokio::test]
async fn execute_tool_suspends_and_resumes() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let module = process_module_with_runner(
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
        registry_with_noop(),
        events.clone(),
    );

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
    assert_eq!(
        *events.lock().expect("events mutex poisoned"),
        vec!["execute".to_string(), r#"resume:"Alice""#.to_string()]
    );
}

#[test]
fn seed_execution_state_maps_positional_arguments_by_prefillable_name() {
    let descriptions = vec![tool_description("interactive", &["name", "unused"])];
    let state = seed_execution_state(&descriptions, "interactive", &[json!("Alice")])
        .expect("state should seed");

    assert_eq!(
        state.interpreter_state.variables.get("name"),
        Some(&json!("Alice"))
    );
    assert!(!state.interpreter_state.variables.contains_key("unused"));
}

#[test]
fn map_execution_state_to_tool_result_serializes_suspensions() {
    let state = crate::execution::state::ExecutionState {
        status: crate::execution::state::ExecutionStatus::Suspended,
        suspension_reason: Some("Need input".to_string()),
        ..Default::default()
    };

    let ToolResult::Suspended(suspension) =
        map_execution_state_to_tool_result("interactive", &state)
            .expect("suspended state should map")
    else {
        panic!("expected suspended result");
    };

    assert_eq!(suspension.reason, "Need input");
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
