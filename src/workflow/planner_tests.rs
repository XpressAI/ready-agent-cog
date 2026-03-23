use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::{ReadyError, Result};
use crate::llm::client::strip_markdown_fences;
use crate::llm::traits::LlmClient;
use crate::plan::Step;
use crate::tools::models::{ToolArgumentDescription, ToolDescription, ToolReturnDescription};
use crate::workflow::planner::SopPlanner;
use crate::workflow::planner::build_description_prompt;
use crate::workflow::planner::build_retry_prompt;
use crate::workflow::planner::parse_and_validate_plan;

struct MockLlm {
    responses: Arc<Mutex<Vec<String>>>,
    calls: Arc<Mutex<Vec<(String, String)>>>,
}

#[async_trait]
impl LlmClient for MockLlm {
    async fn complete(&self, system_prompt: &str, user_prompt: &str) -> Result<String> {
        self.calls
            .lock()
            .expect("calls mutex poisoned")
            .push((system_prompt.to_string(), user_prompt.to_string()));

        let mut responses = self.responses.lock().expect("responses mutex poisoned");
        if responses.is_empty() {
            return Err(ReadyError::Llm("No mock response available".to_string()));
        }
        Ok(responses.remove(0))
    }

    async fn extract(
        &self,
        _system_prompt: &str,
        _user_prompt: &str,
        _json_schema: &Value,
    ) -> Result<Value> {
        Ok(json!({}))
    }
}

fn sample_tools() -> Vec<ToolDescription> {
    vec![
        ToolDescription {
            id: "read_file".to_string(),
            description: "Read a file".to_string(),
            arguments: vec![ToolArgumentDescription {
                name: "file_path".to_string(),
                description: "Path".to_string(),
                type_name: "str".to_string(),
                default: None,
            }],
            returns: ToolReturnDescription {
                name: Some("output".to_string()),
                description: "File contents".to_string(),
                type_name: Some("str".to_string()),
                fields: Vec::new(),
            },
        },
        ToolDescription {
            id: "post_to_slack".to_string(),
            description: "Post to Slack".to_string(),
            arguments: vec![ToolArgumentDescription {
                name: "message".to_string(),
                description: "Message".to_string(),
                type_name: "str".to_string(),
                default: None,
            }],
            returns: ToolReturnDescription {
                name: Some("output".to_string()),
                description: "Success".to_string(),
                type_name: Some("bool".to_string()),
                fields: Vec::new(),
            },
        },
    ]
}

fn planner_with_responses(
    responses: Vec<&str>,
    max_retries: usize,
) -> (SopPlanner, Arc<Mutex<Vec<(String, String)>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let llm = Arc::new(MockLlm {
        responses: Arc::new(Mutex::new(
            responses.into_iter().map(str::to_string).collect(),
        )),
        calls: calls.clone(),
    });
    (SopPlanner::new(llm, max_retries), calls)
}

#[test]
fn strip_markdown_fences_handles_python_generic_and_plain_text() {
    let code = "def main():\n    pass";
    assert_eq!(
        strip_markdown_fences(&format!("```python\n{code}\n```")),
        code
    );
    assert_eq!(strip_markdown_fences(&format!("```\n{code}\n```")), code);
    assert_eq!(strip_markdown_fences(&format!("  {code}  ")), code);
}

#[tokio::test]
async fn plan_calls_llm_and_parses_result() {
    let code = "def main():\n    data = read_file(\"report.txt\")\n    post_to_slack(data)";
    let (planner, calls) = planner_with_responses(vec![code, "Reads a report and posts it."], 3);

    let plan = planner
        .plan("Read a report and post it to Slack", &sample_tools())
        .await
        .expect("planning should succeed");

    assert_eq!(plan.code, code);
    assert_eq!(plan.description, "Reads a report and posts it.");
    assert_eq!(plan.steps.len(), 2);
    assert!(matches!(&plan.steps[0], Step::ToolStep { tool_id, .. } if tool_id == "read_file"));
    assert!(matches!(&plan.steps[1], Step::ToolStep { tool_id, .. } if tool_id == "post_to_slack"));

    let llm_calls = calls.lock().expect("calls mutex poisoned");
    assert_eq!(llm_calls.len(), 2);
    assert!(
        llm_calls[0]
            .0
            .contains("Allowed statements inside `main` are only:")
    );
    assert!(
        llm_calls[1]
            .1
            .contains("Read a report and post it to Slack")
    );
}

#[tokio::test]
async fn plan_retries_after_parse_error_and_appends_error_to_prompt() {
    let valid_code = "def main():\n    data = read_file(\"report.txt\")\n    post_to_slack(data)";
    let (planner, calls) =
        planner_with_responses(vec!["def main(\n    broken", valid_code, "Description."], 3);

    let plan = planner
        .plan("Read report", &sample_tools())
        .await
        .expect("retry should recover");
    assert_eq!(plan.description, "Description.");

    let llm_calls = calls.lock().expect("calls mutex poisoned");
    assert_eq!(llm_calls.len(), 3);
    assert_eq!(llm_calls[0].1, "Read report");
    assert!(llm_calls[1].1.contains("Read report"));
    assert!(llm_calls[1].1.to_lowercase().contains("error"));
}

#[tokio::test]
async fn plan_retries_after_validation_error() {
    let valid_code = "def main():\n    data = read_file(\"report.txt\")\n    post_to_slack(data)";
    let (planner, calls) = planner_with_responses(
        vec![
            "def main():\n    post_to_slack(undefined_var)",
            valid_code,
            "Description.",
        ],
        3,
    );

    let plan = planner
        .plan("Read report", &sample_tools())
        .await
        .expect("retry should recover");
    assert_eq!(plan.steps.len(), 2);
    assert_eq!(calls.lock().expect("calls mutex poisoned").len(), 3);
}

#[test]
fn build_retry_prompt_appends_error_context_and_broken_code() {
    let prompt = build_retry_prompt(
        "Read report",
        "def main():\n    post_to_slack(undefined_var)",
        &ReadyError::Llm("bad output".to_string()),
    );

    assert!(prompt.contains("Read report"));
    assert!(prompt.contains("bad output"));
    assert!(prompt.contains("Previous attempt failed"));
    assert!(prompt.contains("Broken code from the previous attempt"));
    assert!(prompt.contains("post_to_slack(undefined_var)"));
}

#[test]
fn build_description_prompt_lists_prefillable_inputs() {
    let prompt = build_description_prompt(
        "Collect and send a message",
        &[crate::plan::PrefillableInput {
            variable_name: "channel".to_string(),
            prompt: "Which Slack channel?".to_string(),
        }],
    );

    assert!(prompt.contains("Collect and send a message"));
    assert!(prompt.contains("channel"));
    assert!(prompt.contains("Which Slack channel?"));
}

#[test]
fn parse_and_validate_plan_reports_validation_errors_without_llm_flow() {
    let error = parse_and_validate_plan(
        "def main():\n    post_to_slack(undefined_var)",
        "test_plan",
        &sample_tools(),
    )
    .expect_err("invalid plan should fail validation");

    assert!(
        matches!(error, ReadyError::PlanValidation(message) if message.contains("undefined_var"))
    );
}

// Recovery tests
use crate::execution::state::{ExecutionError, ExecutionState, RecoveryContext};
use crate::plan::AbstractPlan;

#[tokio::test]
async fn recover_generates_continuation_plan() {
    let recovery_code = "def main():\n    # Handle error and continue\n    data = read_file(\"report.txt\")\n    post_to_slack(data)";
    let (planner, calls) = planner_with_responses(vec![recovery_code], 3);

    let original_plan = AbstractPlan {
        name: "original_plan".to_string(),
        description: "Original plan".to_string(),
        steps: vec![],
        code: "def main():\n    pass".to_string(),
    };

    let state = ExecutionState::default();
    let error = ExecutionError {
        step_index: Some(5),
        step_type: Some("tool_call".to_string()),
        exception_type: "ToolError".to_string(),
        message: "Connection timeout".to_string(),
    };

    let recovery_context = RecoveryContext::new(original_plan, state, error);

    let plan = planner
        .recover("Read a report and post it to Slack", &recovery_context, &sample_tools())
        .await
        .expect("recovery should succeed");

    assert!(plan.name.contains("recovery"));
    assert_eq!(plan.code, recovery_code);
    assert_eq!(plan.steps.len(), 2);

    let llm_calls = calls.lock().expect("calls mutex poisoned");
    assert_eq!(llm_calls.len(), 1);
    assert!(llm_calls[0].0.contains("recover"));
    assert!(llm_calls[0].1.contains("Original Plan: original_plan"));
    assert!(llm_calls[0].1.contains("State at Error:"));
    assert!(llm_calls[0].1.contains("Error:"));
}

#[tokio::test]
async fn recover_retries_after_parse_error() {
    let valid_code = "def main():\n    data = read_file(\"report.txt\")\n    post_to_slack(data)";
    let (planner, calls) = planner_with_responses(vec!["def main(\n    broken", valid_code], 3);

    let original_plan = AbstractPlan {
        name: "original_plan".to_string(),
        description: "Original plan".to_string(),
        steps: vec![],
        code: "def main():\n    pass".to_string(),
    };

    let state = ExecutionState::default();
    let error = ExecutionError {
        step_index: Some(5),
        step_type: Some("tool_call".to_string()),
        exception_type: "ToolError".to_string(),
        message: "Connection timeout".to_string(),
    };

    let recovery_context = RecoveryContext::new(original_plan, state, error);

    let plan = planner
        .recover("Read a report", &recovery_context, &sample_tools())
        .await
        .expect("retry should recover");

    assert_eq!(plan.code, valid_code);

    let llm_calls = calls.lock().expect("calls mutex poisoned");
    assert_eq!(llm_calls.len(), 2);
    assert!(llm_calls[1].1.contains("Previous attempt failed"));
}

#[tokio::test]
async fn recover_retries_after_validation_error() {
    let valid_code = "def main():\n    data = read_file(\"report.txt\")\n    post_to_slack(data)";
    let (planner, calls) = planner_with_responses(
        vec![
            "def main():\n    post_to_slack(undefined_var)",
            valid_code,
        ],
        3,
    );

    let original_plan = AbstractPlan {
        name: "original_plan".to_string(),
        description: "Original plan".to_string(),
        steps: vec![],
        code: "def main():\n    pass".to_string(),
    };

    let state = ExecutionState::default();
    let error = ExecutionError {
        step_index: Some(5),
        step_type: Some("tool_call".to_string()),
        exception_type: "ToolError".to_string(),
        message: "Connection timeout".to_string(),
    };

    let recovery_context = RecoveryContext::new(original_plan, state, error);

    let plan = planner
        .recover("Read a report", &recovery_context, &sample_tools())
        .await
        .expect("retry should recover");

    assert_eq!(plan.steps.len(), 2);
    assert_eq!(calls.lock().expect("calls mutex poisoned").len(), 2);
}

#[tokio::test]
async fn recover_fails_after_max_retries() {
    let (planner, _) = planner_with_responses(vec!["def main(\n    broken"], 2);

    let original_plan = AbstractPlan {
        name: "original_plan".to_string(),
        description: "Original plan".to_string(),
        steps: vec![],
        code: "def main():\n    pass".to_string(),
    };

    let state = ExecutionState::default();
    let error = ExecutionError {
        step_index: Some(5),
        step_type: Some("tool_call".to_string()),
        exception_type: "ToolError".to_string(),
        message: "Connection timeout".to_string(),
    };

    let recovery_context = RecoveryContext::new(original_plan, state, error);

    let result = planner
        .recover("Read a report", &recovery_context, &sample_tools())
        .await;

    assert!(result.is_err());
}

// RecoveryPlanner trait implementation tests
use crate::workflow::executor::RecoveryPlanner;

#[tokio::test]
async fn sop_planner_implements_recovery_planner_trait() {
    let recovery_code = "def main():\n    data = read_file(\"report.txt\")\n    post_to_slack(data)";
    let (planner, calls) = planner_with_responses(vec![recovery_code], 3);

    let original_plan = AbstractPlan {
        name: "original_plan".to_string(),
        description: "Original plan".to_string(),
        steps: vec![],
        code: "def main():\n    pass".to_string(),
    };

    let state = ExecutionState::default();
    let error = ExecutionError {
        step_index: Some(5),
        step_type: Some("tool_call".to_string()),
        exception_type: "ToolError".to_string(),
        message: "Connection timeout".to_string(),
    };

    let recovery_context = RecoveryContext::new(original_plan, state, error);

    // Test that SopPlanner implements RecoveryPlanner trait
    let plan = planner
        .recover("Read a report and post it to Slack", &recovery_context, &sample_tools())
        .await
        .expect("recovery should succeed via trait");

    assert!(plan.name.contains("recovery"));
    assert_eq!(plan.code, recovery_code);

    let llm_calls = calls.lock().expect("calls mutex poisoned");
    assert_eq!(llm_calls.len(), 1);
}

#[tokio::test]
async fn sop_planner_trait_can_be_used_as_dyn_recovery_planner() {
    let recovery_code = "def main():\n    data = read_file(\"report.txt\")\n    post_to_slack(data)";
    let (planner, calls) = planner_with_responses(vec![recovery_code], 3);

    let original_plan = AbstractPlan {
        name: "original_plan".to_string(),
        description: "Original plan".to_string(),
        steps: vec![],
        code: "def main():\n    pass".to_string(),
    };

    let state = ExecutionState::default();
    let error = ExecutionError {
        step_index: Some(5),
        step_type: Some("tool_call".to_string()),
        exception_type: "ToolError".to_string(),
        message: "Connection timeout".to_string(),
    };

    let recovery_context = RecoveryContext::new(original_plan, state, error);

    // Test that SopPlanner can be used as &dyn RecoveryPlanner
    let planner_ref: &dyn RecoveryPlanner = &planner;
    let plan = planner_ref
        .recover("Read a report and post it to Slack", &recovery_context, &sample_tools())
        .await
        .expect("recovery should succeed via trait object");

    assert!(plan.name.contains("recovery"));
    assert_eq!(plan.code, recovery_code);

    let llm_calls = calls.lock().expect("calls mutex poisoned");
    assert_eq!(llm_calls.len(), 1);
}
