use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::execution::state::ExecutionStatus;
use crate::llm::traits::LlmClient;
use crate::plan::{AbstractPlan, Expression, Step};
use crate::test_helpers::{HandlerToolsModule, to_literal};
use crate::tools::builtin::BuiltinToolsModule;
use crate::tools::models::{ToolDescription, ToolResult};
use crate::tools::registry::InMemoryToolRegistry;
use crate::workflow::executor::SopExecutor;

struct MockLlm;

#[async_trait]
impl LlmClient for MockLlm {
    async fn complete(
        &self,
        _system_prompt: &str,
        _user_prompt: &str,
    ) -> crate::error::Result<String> {
        Ok(String::new())
    }

    async fn extract(
        &self,
        _system_prompt: &str,
        _user_prompt: &str,
        _json_schema: &Value,
    ) -> crate::error::Result<Value> {
        Ok(Value::Null)
    }
}

fn registry_with_send(sent: Arc<Mutex<Vec<String>>>) -> Arc<InMemoryToolRegistry> {
    let mut registry = InMemoryToolRegistry::new();
    registry
        .register_module(Box::new(BuiltinToolsModule::new(Arc::new(MockLlm))))
        .unwrap();
    registry
        .register_module(Box::new(HandlerToolsModule::new(
            vec![ToolDescription {
                id: "send".to_string(),
                description: "Send a message".to_string(),
                arguments: vec![crate::tools::models::ToolArgumentDescription {
                    name: "message".to_string(),
                    description: String::new(),
                    type_name: "str".to_string(),
                    default: None,
                }],
                returns: crate::tools::models::ToolReturnDescription {
                    name: None,
                    description: String::new(),
                    type_name: None,
                    fields: Vec::new(),
                },
            }],
            move |_tool_id, args| {
                sent.lock().expect("sent mutex poisoned").push(
                    args[0]
                        .as_str()
                        .expect("message should be a string")
                        .to_string(),
                );
                Ok(ToolResult::Success(Value::Null))
            },
        )))
        .unwrap();
    Arc::new(registry)
}

fn literal(value: Value) -> Expression {
    Expression::Literal {
        value: to_literal(value),
    }
}

fn access(name: &str) -> Expression {
    Expression::AccessPath {
        variable_name: name.to_string(),
        accessors: Vec::new(),
    }
}

#[derive(Clone)]
struct StubPlanRunner {
    events: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl super::executor::PlanRunner for StubPlanRunner {
    async fn execute(
        &self,
        _registry: Arc<InMemoryToolRegistry>,
        _observer: Arc<dyn crate::execution::observer::ExecutionObserver>,
        _plan: &AbstractPlan,
        state: &mut crate::execution::state::ExecutionState,
    ) -> crate::error::Result<()> {
        self.events
            .lock()
            .expect("events mutex poisoned")
            .push("execute".to_string());

        if let Some(value) = state.interpreter_state.variables.get("user_input").cloned() {
            state
                .interpreter_state
                .variables
                .insert("sent_message".to_string(), value);
            state.status = ExecutionStatus::Completed;
        } else {
            state.status = ExecutionStatus::Suspended;
            state.suspension_reason = Some("Enter your input:".to_string());
        }
        Ok(())
    }

    async fn provide_input(
        &self,
        _registry: Arc<InMemoryToolRegistry>,
        _observer: Arc<dyn crate::execution::observer::ExecutionObserver>,
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
            .insert("user_input".to_string(), value.clone());
        state
            .interpreter_state
            .variables
            .insert("sent_message".to_string(), value);
        state.status = ExecutionStatus::Completed;
        state.suspension_reason = None;
        Ok(())
    }
}

fn executor_with_stub_runner(events: Arc<Mutex<Vec<String>>>) -> SopExecutor {
    SopExecutor::with_runner(
        Arc::new(InMemoryToolRegistry::new()),
        None,
        Arc::new(StubPlanRunner { events }),
    )
}

#[tokio::test]
async fn execute_runs_simple_plan_to_completion() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let registry = registry_with_send(sent.clone());
    let executor = SopExecutor::new(registry, None);

    let plan = AbstractPlan {
        name: "simple".to_string(),
        description: String::new(),
        steps: vec![Step::ToolStep {
            tool_id: "send".to_string(),
            arguments: vec![literal(json!("hello"))],
            output_variable: None,
        }],
        code: String::new(),
    };

    let state = executor
        .execute(&plan, HashMap::new(), None)
        .await
        .expect("plan should execute");
    assert_eq!(state.status, ExecutionStatus::Completed);
    assert_eq!(
        *sent.lock().expect("sent mutex poisoned"),
        vec!["hello".to_string()]
    );
}

#[tokio::test]
async fn execute_returns_suspended_state_and_resume_completes() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let executor = executor_with_stub_runner(events.clone());

    let plan = AbstractPlan {
        name: "interactive".to_string(),
        description: String::new(),
        steps: vec![
            Step::ToolStep {
                tool_id: "send".to_string(),
                arguments: vec![literal(json!("start"))],
                output_variable: None,
            },
            Step::UserInteractionStep {
                prompt: "Enter your input:".to_string(),
                output_variable: Some("user_input".to_string()),
            },
            Step::ToolStep {
                tool_id: "send".to_string(),
                arguments: vec![access("user_input")],
                output_variable: None,
            },
        ],
        code: String::new(),
    };

    let mut state = executor
        .execute(&plan, HashMap::new(), None)
        .await
        .expect("execution should suspend");
    assert_eq!(state.status, ExecutionStatus::Suspended);
    assert_eq!(
        state.suspension_reason.as_deref(),
        Some("Enter your input:")
    );

    executor
        .resume(&plan, &mut state, json!("user_answer"))
        .await
        .expect("resume should complete");
    assert_eq!(state.status, ExecutionStatus::Completed);
    assert_eq!(
        state.interpreter_state.variables.get("sent_message"),
        Some(&json!("user_answer"))
    );
    assert_eq!(
        *events.lock().expect("events mutex poisoned"),
        vec!["execute".to_string(), r#"resume:"user_answer""#.to_string()]
    );
}

#[tokio::test]
async fn execute_seeds_initial_inputs_and_skips_user_interaction() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let executor = executor_with_stub_runner(events.clone());

    let plan = AbstractPlan {
        name: "seeded".to_string(),
        description: String::new(),
        steps: vec![
            Step::UserInteractionStep {
                prompt: "Enter your input:".to_string(),
                output_variable: Some("user_input".to_string()),
            },
            Step::ToolStep {
                tool_id: "send".to_string(),
                arguments: vec![access("user_input")],
                output_variable: None,
            },
        ],
        code: String::new(),
    };

    let state = executor
        .execute(
            &plan,
            HashMap::from([("user_input".to_string(), json!("pre-filled"))]),
            None,
        )
        .await
        .expect("seeded plan should execute");

    assert_eq!(state.status, ExecutionStatus::Completed);
    assert_eq!(
        state.interpreter_state.variables.get("sent_message"),
        Some(&json!("pre-filled"))
    );
    assert_eq!(
        *events.lock().expect("events mutex poisoned"),
        vec!["execute".to_string()]
    );
}
