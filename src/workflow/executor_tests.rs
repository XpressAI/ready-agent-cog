use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::execution::state::ExecutionStatus;
use crate::llm::traits::LlmClient;
use crate::plan::{AbstractPlan, Expression, Step};
use crate::tools::builtin::BuiltinToolsModule;
use crate::tools::models::{ToolCall, ToolDescription, ToolResult};
use crate::tools::registry::InMemoryToolRegistry;
use crate::tools::traits::ToolsModule;
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

struct SendModule {
    sent: Arc<Mutex<Vec<String>>>,
    tools: Vec<ToolDescription>,
}

impl SendModule {
    fn new(sent: Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            sent,
            tools: vec![ToolDescription {
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
        }
    }
}

impl ToolsModule for SendModule {
    fn tools(&self) -> &[ToolDescription] {
        &self.tools
    }

    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
    ) -> Pin<Box<dyn std::future::Future<Output = crate::error::Result<ToolResult>> + Send + 'a>>
    {
        self.sent.lock().expect("sent mutex poisoned").push(
            call.args[0]
                .as_str()
                .expect("message should be a string")
                .to_string(),
        );
        Box::pin(async move { Ok(ToolResult::Success(Value::Null)) })
    }
}

fn registry_with_send(sent: Arc<Mutex<Vec<String>>>) -> Arc<InMemoryToolRegistry> {
    let mut registry = InMemoryToolRegistry::new();
    registry
        .register_module(Box::new(BuiltinToolsModule::new(Arc::new(MockLlm))))
        .unwrap();
    registry
        .register_module(Box::new(SendModule::new(sent)))
        .unwrap();
    Arc::new(registry)
}

fn literal(value: Value) -> Expression {
    fn to_literal(value: Value) -> crate::plan::LiteralValue {
        match value {
            Value::Null => crate::plan::LiteralValue::Null,
            Value::Bool(value) => crate::plan::LiteralValue::Bool(value),
            Value::Number(number) => {
                if let Some(value) = number.as_i64() {
                    crate::plan::LiteralValue::Integer(value)
                } else {
                    crate::plan::LiteralValue::Float(number.as_f64().expect("finite json number"))
                }
            }
            Value::String(value) => crate::plan::LiteralValue::String(value),
            Value::Array(values) => {
                crate::plan::LiteralValue::Array(values.into_iter().map(to_literal).collect())
            }
            Value::Object(values) => crate::plan::LiteralValue::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, to_literal(value)))
                    .collect(),
            ),
        }
    }

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
    let sent = Arc::new(Mutex::new(Vec::new()));
    let registry = registry_with_send(sent.clone());
    let executor = SopExecutor::new(registry, None);

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
        *sent.lock().expect("sent mutex poisoned"),
        vec!["start".to_string(), "user_answer".to_string()]
    );
}

#[tokio::test]
async fn execute_seeds_initial_inputs_and_skips_user_interaction() {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let registry = registry_with_send(sent.clone());
    let executor = SopExecutor::new(registry, None);

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
        *sent.lock().expect("sent mutex poisoned"),
        vec!["pre-filled".to_string()]
    );
}
