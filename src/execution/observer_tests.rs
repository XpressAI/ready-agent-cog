use super::observer::{ExecutionObserver, LoggingObserver, NoOpObserver};
use super::state::{ExecutionState, ExecutionStatus, StepResult};
use crate::error::ReadyError;
use crate::plan::{AbstractPlan, Expression, Step};
use crate::test_helpers::{HandlerToolsModule, to_literal};
use crate::tools::models::{ToolDescription, ToolResult, ToolReturnDescription};
use crate::tools::registry::InMemoryToolRegistry;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Default, Clone)]
struct SpyObserver {
    events: Arc<Mutex<Vec<(String, String)>>>,
}

impl SpyObserver {
    fn event_names(&self) -> Vec<String> {
        self.events
            .lock()
            .expect("events mutex poisoned")
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    }

    fn snapshot(&self) -> Vec<(String, String)> {
        self.events.lock().expect("events mutex poisoned").clone()
    }
}

impl ExecutionObserver for SpyObserver {
    fn on_plan_start(&self, plan_name: &str, _state: &ExecutionState) {
        self.events
            .lock()
            .expect("events mutex poisoned")
            .push(("plan_start".to_string(), plan_name.to_string()));
    }

    fn on_step_start(
        &self,
        step_index: usize,
        _step: &Step,
        _ip: &[usize],
        _state: &ExecutionState,
    ) {
        self.events
            .lock()
            .expect("events mutex poisoned")
            .push(("step_start".to_string(), step_index.to_string()));
    }

    fn on_step_complete(&self, step_index: usize, _step: &Step, _result: &StepResult) {
        self.events
            .lock()
            .expect("events mutex poisoned")
            .push(("step_complete".to_string(), step_index.to_string()));
    }

    fn on_suspension(&self, reason: &str, _state: &ExecutionState) {
        self.events
            .lock()
            .expect("events mutex poisoned")
            .push(("suspension".to_string(), reason.to_string()));
    }

    fn on_error(&self, _step: &Step, error: &ReadyError, _state: &ExecutionState) {
        self.events
            .lock()
            .expect("events mutex poisoned")
            .push(("error".to_string(), error.to_string()));
    }

    fn on_plan_complete(&self, state: &ExecutionState) {
        self.events
            .lock()
            .expect("events mutex poisoned")
            .push(("plan_complete".to_string(), format!("{:?}", state.status)));
    }
}

fn tool_description(tool_id: &str) -> ToolDescription {
    ToolDescription {
        id: tool_id.to_string(),
        description: String::new(),
        arguments: Vec::new(),
        returns: ToolReturnDescription {
            name: None,
            description: String::new(),
            type_name: None,
            fields: Vec::new(),
        },
    }
}

fn registry_with_handler(
    tools: Vec<ToolDescription>,
    handler: impl Fn(&str, Vec<Value>) -> crate::error::Result<ToolResult> + Send + Sync + 'static,
) -> InMemoryToolRegistry {
    let mut registry = InMemoryToolRegistry::new();
    registry
        .register_module(Box::new(HandlerToolsModule::new(tools, handler)))
        .unwrap();
    registry
}

fn assign(var: &str, value: Value) -> Step {
    Step::AssignStep {
        target: var.to_string(),
        value: Expression::Literal {
            value: to_literal(value),
        },
    }
}

fn make_plan(steps: Vec<Step>, name: &str) -> AbstractPlan {
    AbstractPlan {
        name: name.to_string(),
        description: String::new(),
        steps,
        code: String::new(),
    }
}

fn assert_execution_observer<T: ExecutionObserver>(_observer: &T) {}

#[test]
fn noop_observer_satisfies_protocol() {
    let observer = NoOpObserver;
    assert_execution_observer(&observer);
}

#[test]
fn logging_observer_satisfies_protocol() {
    let observer = LoggingObserver;
    assert_execution_observer(&observer);
}

#[tokio::test]
async fn observer_fires_lifecycle_events() {
    let spy = SpyObserver::default();
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = make_plan(vec![assign("x", json!(1)), assign("y", json!(2))], "main");
    let interpreter = super::interpreter::PlanInterpreter::new(registry, plan.clone())
        .with_observer(Arc::new(spy.clone()));
    let mut state = ExecutionState::default();

    interpreter
        .execute(&mut state)
        .await
        .expect("plan should execute");

    let events = spy.snapshot();
    assert_eq!(state.status, ExecutionStatus::Completed);
    assert_eq!(
        events.first(),
        Some(&("plan_start".to_string(), "main".to_string()))
    );
    assert!(events.contains(&("step_start".to_string(), "0".to_string())));
    assert!(events.contains(&("step_complete".to_string(), "0".to_string())));
    assert!(events.contains(&("step_start".to_string(), "1".to_string())));
    assert!(events.contains(&("step_complete".to_string(), "1".to_string())));
    assert_eq!(
        events.last(),
        Some(&("plan_complete".to_string(), "Completed".to_string()))
    );
}

#[tokio::test]
async fn observer_fires_lifecycle_events_in_correct_order() {
    let spy = SpyObserver::default();
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = make_plan(vec![assign("x", json!(42))], "main");
    let interpreter = super::interpreter::PlanInterpreter::new(registry, plan.clone())
        .with_observer(Arc::new(spy.clone()));
    let mut state = ExecutionState::default();

    interpreter
        .execute(&mut state)
        .await
        .expect("plan should execute");

    let event_names = spy.event_names();
    assert_eq!(event_names[0], "plan_start");
    assert_eq!(event_names[1], "step_start");
    assert_eq!(event_names[2], "step_complete");
    assert_eq!(event_names.last(), Some(&"plan_complete".to_string()));
}

#[tokio::test]
async fn observer_fires_on_error() {
    let spy = SpyObserver::default();
    let registry = Arc::new(registry_with_handler(
        vec![tool_description("boom_tool")],
        |_tool_id, _args| {
            Err(ReadyError::Execution {
                step_index: None,
                step_type: Some("ToolStep".to_string()),
                message: "intentional failure".to_string(),
            })
        },
    ));
    let plan = make_plan(
        vec![Step::ToolStep {
            tool_id: "boom_tool".to_string(),
            arguments: Vec::new(),
            output_variable: None,
        }],
        "main",
    );
    let interpreter = super::interpreter::PlanInterpreter::new(registry, plan.clone())
        .with_observer(Arc::new(spy.clone()));
    let mut state = ExecutionState::default();

    let _ = interpreter.execute(&mut state).await;

    let events = spy.snapshot();
    assert_eq!(state.status, ExecutionStatus::Failed);
    assert_eq!(events.iter().filter(|(name, _)| name == "error").count(), 1);
    assert_eq!(
        events.last(),
        Some(&("plan_complete".to_string(), "Failed".to_string()))
    );
}

#[tokio::test]
async fn no_observer_uses_noop_by_default() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = make_plan(vec![assign("x", json!(99))], "main");
    let interpreter = super::interpreter::PlanInterpreter::new(registry, plan.clone());
    let mut state = ExecutionState::default();

    interpreter
        .execute(&mut state)
        .await
        .expect("plan should execute");

    assert_eq!(state.status, ExecutionStatus::Completed);
    assert_eq!(state.interpreter_state.variables.get("x"), Some(&json!(99)));
}
