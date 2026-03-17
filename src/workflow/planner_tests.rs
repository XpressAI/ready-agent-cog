use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::error::{ReadyError, Result};
use crate::llm::client::strip_markdown_fences;
use crate::llm::traits::LlmClient;
use crate::plan::{DiagnosticSeverity, Step};
use crate::tools::models::{ToolArgumentDescription, ToolDescription, ToolReturnDescription};
use crate::workflow::planner::SopPlanner;

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

#[tokio::test]
async fn validate_plan_surfaces_hard_errors_for_undefined_variable_case() {
    let plan = crate::planning::parser::parse_python_to_plan(
        "def main():\n    post_to_slack(undefined_var)",
        "test_plan",
    )
    .expect("plan should parse");
    let issues = crate::planning::validator::validate_plan(&plan, &sample_tools());
    assert!(
        issues
            .iter()
            .any(|issue| issue.severity == DiagnosticSeverity::Error)
    );
}
