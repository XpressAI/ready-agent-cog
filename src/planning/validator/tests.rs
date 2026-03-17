use super::*;

use crate::plan::{BranchKind, ConditionalBranch, Expression, Step};
use crate::test_helpers::{assign, lit, plan, tool, var};

fn tools(ids: &[&str]) -> Vec<crate::tools::models::ToolDescription> {
    ids.iter().map(|id| tool(id)).collect()
}

#[test]
fn no_errors_when_all_variables_defined_before_use() {
    let plan = plan(vec![
        assign("greeting", lit("hello")),
        Step::ToolStep {
            tool_id: "send".to_string(),
            arguments: vec![var("greeting")],
            output_variable: None,
        },
    ]);
    assert_eq!(
        validate_plan(&plan, &tools(&["send"])),
        Vec::<PlanDiagnostic>::new()
    );
}

#[test]
fn error_for_reference_to_never_defined_variable() {
    let plan = plan(vec![Step::ToolStep {
        tool_id: "send".to_string(),
        arguments: vec![var("unknown_var")],
        output_variable: None,
    }]);
    let diagnostics = validate_plan(&plan, &tools(&["send"]));
    let errors = diagnostics
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .collect::<Vec<_>>();
    assert!(!errors.is_empty());
    assert!(
        errors
            .iter()
            .any(|d| d.variable_name.as_deref() == Some("unknown_var"))
    );
    assert!(errors.iter().any(|d| d.message.contains("not defined")));
}

#[test]
fn error_for_variable_used_before_definition() {
    let plan = plan(vec![
        Step::ToolStep {
            tool_id: "send".to_string(),
            arguments: vec![var("x")],
            output_variable: None,
        },
        assign("x", lit("hello")),
    ]);
    let diagnostics = validate_plan(&plan, &tools(&["send"]));
    let errors = diagnostics
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .collect::<Vec<_>>();
    assert!(
        errors
            .iter()
            .any(|d| d.variable_name.as_deref() == Some("x"))
    );
}

#[test]
fn warning_for_variable_assigned_but_never_used() {
    let plan = plan(vec![
        assign("unused", lit("hello")),
        assign("used", lit("world")),
        Step::ToolStep {
            tool_id: "send".to_string(),
            arguments: vec![var("used")],
            output_variable: None,
        },
    ]);
    let result = validate_plan(&plan, &tools(&["send"]));
    let warnings = result
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Warning)
        .collect::<Vec<_>>();
    assert!(!warnings.is_empty());
    assert!(
        warnings
            .iter()
            .any(|d| d.variable_name.as_deref() == Some("unused"))
    );
}

#[test]
fn last_step_output_is_not_flagged_as_unused() {
    let plan = plan(vec![
        assign("x", lit("hello")),
        Step::ToolStep {
            tool_id: "compute".to_string(),
            arguments: vec![var("x")],
            output_variable: Some("final_result".to_string()),
        },
    ]);
    let result = validate_plan(&plan, &tools(&["compute"]));
    assert!(
        !result
            .iter()
            .any(|diagnostic| diagnostic.variable_name.as_deref() == Some("final_result"))
    );
}

#[test]
fn outer_variable_available_inside_switch_branch() {
    let plan = plan(vec![
        assign("x", lit("data")),
        Step::SwitchStep {
            branches: vec![
                ConditionalBranch {
                    kind: BranchKind::If,
                    condition: Some(var("x")),
                    steps: vec![Step::ToolStep {
                        tool_id: "process".to_string(),
                        arguments: vec![var("x")],
                        output_variable: None,
                    }],
                },
                ConditionalBranch {
                    kind: BranchKind::Else,
                    condition: None,
                    steps: vec![Step::ToolStep {
                        tool_id: "skip".to_string(),
                        arguments: vec![],
                        output_variable: None,
                    }],
                },
            ],
        },
    ]);
    let diagnostics = validate_plan(&plan, &tools(&["process", "skip"]));
    let errors = diagnostics
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .collect::<Vec<_>>();
    assert!(errors.is_empty());
}

#[test]
fn variable_defined_inside_branch_not_available_after_scope() {
    let plan = plan(vec![
        Step::SwitchStep {
            branches: vec![ConditionalBranch {
                kind: BranchKind::If,
                condition: Some(lit(true)),
                steps: vec![assign("inner_var", lit("inner"))],
            }],
        },
        Step::ToolStep {
            tool_id: "use".to_string(),
            arguments: vec![var("inner_var")],
            output_variable: None,
        },
    ]);
    let diagnostics = validate_plan(&plan, &tools(&["use"]));
    let errors = diagnostics
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .collect::<Vec<_>>();
    assert!(
        errors
            .iter()
            .any(|d| d.variable_name.as_deref() == Some("inner_var"))
    );
}

#[test]
fn loop_iterable_variable_undefined() {
    let plan = plan(vec![Step::LoopStep {
        iterable_variable: "missing_items".to_string(),
        item_variable: "item".to_string(),
        body: vec![Step::ToolStep {
            tool_id: "process".to_string(),
            arguments: vec![var("item")],
            output_variable: None,
        }],
    }]);
    let diagnostics = validate_plan(&plan, &tools(&["process"]));
    let errors = diagnostics
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .collect::<Vec<_>>();
    assert!(
        errors
            .iter()
            .any(|d| d.variable_name.as_deref() == Some("missing_items"))
    );
}

#[test]
fn loop_iterable_variable_defined() {
    let plan = plan(vec![
        assign("items", lit(vec![1, 2, 3])),
        Step::LoopStep {
            iterable_variable: "items".to_string(),
            item_variable: "item".to_string(),
            body: vec![Step::ToolStep {
                tool_id: "process".to_string(),
                arguments: vec![var("item")],
                output_variable: None,
            }],
        },
    ]);
    let diagnostics = validate_plan(&plan, &tools(&["process"]));
    let errors = diagnostics
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .collect::<Vec<_>>();
    assert!(errors.is_empty());
}

#[test]
fn item_variable_is_available_inside_loop_body() {
    let plan = plan(vec![
        assign("items", lit(vec![1, 2, 3])),
        Step::LoopStep {
            iterable_variable: "items".to_string(),
            item_variable: "item".to_string(),
            body: vec![Step::ToolStep {
                tool_id: "process".to_string(),
                arguments: vec![var("item")],
                output_variable: None,
            }],
        },
    ]);
    let diagnostics = validate_plan(&plan, &tools(&["process"]));
    let errors = diagnostics
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .collect::<Vec<_>>();
    assert!(errors.is_empty());
}

#[test]
fn outer_variable_available_inside_loop_body() {
    let plan = plan(vec![
        assign("cfg", lit("config")),
        assign("items", lit(vec![1, 2])),
        Step::LoopStep {
            iterable_variable: "items".to_string(),
            item_variable: "item".to_string(),
            body: vec![Step::ToolStep {
                tool_id: "process".to_string(),
                arguments: vec![var("item"), var("cfg")],
                output_variable: None,
            }],
        },
    ]);
    let diagnostics = validate_plan(&plan, &tools(&["process"]));
    let errors = diagnostics
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .collect::<Vec<_>>();
    assert!(errors.is_empty());
}

#[test]
fn variable_from_user_interaction_is_available_for_subsequent_steps() {
    let plan = plan(vec![
        Step::UserInteractionStep {
            prompt: "Please provide the filepath to the jargon file:".to_string(),
            output_variable: Some("jargon_file_path".to_string()),
        },
        Step::UserInteractionStep {
            prompt: "Please provide the URL to the meeting transcript:".to_string(),
            output_variable: Some("transcript_url".to_string()),
        },
        Step::ToolStep {
            tool_id: "read_google_doc".to_string(),
            arguments: vec![var("transcript_url")],
            output_variable: Some("meeting_text".to_string()),
        },
        Step::ToolStep {
            tool_id: "read_google_doc".to_string(),
            arguments: vec![var("transcript_url")],
            output_variable: Some("meeting_text".to_string()),
        },
        assign(
            "system_prompt",
            Expression::ConcatExpression {
                parts: vec![lit("System prompt prefix: "), var("jargon_file_path")],
            },
        ),
        assign("user_prompt", var("meeting_text")),
        Step::ToolStep {
            tool_id: "delegate_to_large_language_model".to_string(),
            arguments: vec![var("system_prompt"), var("user_prompt")],
            output_variable: Some("summary".to_string()),
        },
        Step::ToolStep {
            tool_id: "post_to_slack".to_string(),
            arguments: vec![var("summary")],
            output_variable: None,
        },
    ]);
    let diagnostics = validate_plan(
        &plan,
        &tools(&[
            "read_google_doc",
            "delegate_to_large_language_model",
            "post_to_slack",
        ]),
    );
    let errors = diagnostics
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .collect::<Vec<_>>();
    assert!(
        errors.is_empty(),
        "Expected no errors but got: {:?}",
        diagnostics
    );
}

#[test]
fn user_interaction_output_variable_can_be_used_in_same_scope() {
    let plan = plan(vec![
        Step::UserInteractionStep {
            prompt: "Enter value:".to_string(),
            output_variable: Some("my_var".to_string()),
        },
        Step::ToolStep {
            tool_id: "use_it".to_string(),
            arguments: vec![var("my_var")],
            output_variable: None,
        },
    ]);
    let diagnostics = validate_plan(&plan, &tools(&["use_it"]));
    let errors = diagnostics
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .collect::<Vec<_>>();
    assert!(errors.is_empty());
}

#[test]
fn user_interaction_with_none_output_does_not_define_variable() {
    let plan = plan(vec![
        Step::UserInteractionStep {
            prompt: "Acknowledge:".to_string(),
            output_variable: None,
        },
        Step::ToolStep {
            tool_id: "next".to_string(),
            arguments: vec![],
            output_variable: None,
        },
    ]);
    let diagnostics = validate_plan(&plan, &tools(&["next"]));
    let errors = diagnostics
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .collect::<Vec<_>>();
    assert!(errors.is_empty());
}

#[test]
fn empty_steps_produces_no_errors() {
    let plan = plan(vec![]);
    assert_eq!(validate_plan(&plan, &[]), Vec::<PlanDiagnostic>::new());
}

#[test]
fn access_path_base_tracked() {
    use serde_json::json;
    let plan = plan(vec![
        assign("data", lit(json!({"x": 1}))),
        Step::ToolStep {
            tool_id: "use".to_string(),
            arguments: vec![Expression::AccessPath {
                variable_name: "data".to_string(),
                accessors: vec![],
            }],
            output_variable: None,
        },
    ]);
    let errors = validate_plan(&plan, &tools(&["use"]))
        .into_iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .collect::<Vec<_>>();
    assert!(errors.is_empty());
}

#[test]
fn access_path_undefined_base_error() {
    let plan = plan(vec![Step::ToolStep {
        tool_id: "use".to_string(),
        arguments: vec![Expression::AccessPath {
            variable_name: "missing_base".to_string(),
            accessors: vec![],
        }],
        output_variable: None,
    }]);
    let diagnostics = validate_plan(&plan, &tools(&["use"]));
    let errors = diagnostics
        .iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .collect::<Vec<_>>();
    assert!(
        errors
            .iter()
            .any(|d| d.variable_name.as_deref() == Some("missing_base"))
    );
}

#[test]
fn unknown_tool_produces_error_diagnostic() {
    let plan = plan(vec![Step::ToolStep {
        tool_id: "unknown".to_string(),
        arguments: Vec::new(),
        output_variable: None,
    }]);

    let diagnostics = validate_plan(&plan, &[]);
    assert!(diagnostics.iter().any(|d| {
        d.severity == DiagnosticSeverity::Error && d.message.contains("Unknown tool_id 'unknown'")
    }));
}
