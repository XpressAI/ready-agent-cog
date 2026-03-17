use super::*;

use crate::planning::validator::validate_plan;
use crate::test_helpers::{lit, plan, tool, var};

fn roundtrip(plan: &AbstractPlan) -> AbstractPlan {
    serde_json::from_str(&serde_json::to_string(plan).expect("plan should serialize"))
        .expect("plan should deserialize")
}

#[test]
fn tool_step_fields_preserved() {
    let plan = plan(vec![Step::ToolStep {
        tool_id: "fetch_data".to_string(),
        arguments: vec![lit("url")],
        output_variable: Some("content".to_string()),
    }]);
    let rt = roundtrip(&plan);
    let Step::ToolStep {
        tool_id,
        arguments,
        output_variable,
    } = &rt.steps[0]
    else {
        panic!("expected tool step");
    };
    assert_eq!(tool_id, "fetch_data");
    assert_eq!(output_variable.as_deref(), Some("content"));
    assert_eq!(arguments[0], lit("url"));
}

#[test]
fn user_interaction_step_fields_preserved() {
    let plan = plan(vec![Step::UserInteractionStep {
        prompt: "What is your name?".to_string(),
        output_variable: Some("name".to_string()),
    }]);
    let rt = roundtrip(&plan);
    let Step::UserInteractionStep {
        prompt,
        output_variable,
    } = &rt.steps[0]
    else {
        panic!("expected user interaction step");
    };
    assert_eq!(prompt, "What is your name?");
    assert_eq!(output_variable.as_deref(), Some("name"));
}

#[test]
fn access_path_argument_preserved() {
    let plan = plan(vec![Step::ToolStep {
        tool_id: "use".to_string(),
        arguments: vec![var("my_var")],
        output_variable: None,
    }]);
    let rt = roundtrip(&plan);
    assert_eq!(
        rt.steps,
        vec![Step::ToolStep {
            tool_id: "use".to_string(),
            arguments: vec![var("my_var")],
            output_variable: None,
        }]
    );
}

#[test]
fn steps_inside_switch_branch_are_concrete() {
    let plan = plan(vec![Step::SwitchStep {
        branches: vec![ConditionalBranch {
            kind: BranchKind::If,
            condition: Some(lit(true)),
            steps: vec![
                Step::UserInteractionStep {
                    prompt: "Nested prompt?".to_string(),
                    output_variable: Some("ans".to_string()),
                },
                Step::ToolStep {
                    tool_id: "use".to_string(),
                    arguments: vec![var("ans")],
                    output_variable: None,
                },
            ],
        }],
    }]);
    let rt = roundtrip(&plan);
    let Step::SwitchStep { branches } = &rt.steps[0] else {
        panic!("expected switch step");
    };
    assert!(matches!(
        branches[0].steps[0],
        Step::UserInteractionStep { .. }
    ));
    assert!(matches!(branches[0].steps[1], Step::ToolStep { .. }));
}

#[test]
fn steps_inside_loop_body_are_concrete() {
    let plan = plan(vec![Step::LoopStep {
        iterable_variable: "items".to_string(),
        item_variable: "item".to_string(),
        body: vec![
            Step::ToolStep {
                tool_id: "process".to_string(),
                arguments: vec![var("item")],
                output_variable: Some("out".to_string()),
            },
            Step::AssignStep {
                target: "done".to_string(),
                value: lit(true),
            },
        ],
    }]);
    let rt = roundtrip(&plan);
    let Step::LoopStep { body, .. } = &rt.steps[0] else {
        panic!("expected loop step");
    };
    assert!(matches!(body[0], Step::ToolStep { .. }));
    assert!(matches!(body[1], Step::AssignStep { .. }));
}

#[test]
fn steps_inside_while_body_are_concrete() {
    let plan = plan(vec![Step::WhileStep {
        condition: var("running"),
        body: vec![Step::AssignStep {
            target: "running".to_string(),
            value: lit(false),
        }],
    }]);
    let rt = roundtrip(&plan);
    let Step::WhileStep { body, .. } = &rt.steps[0] else {
        panic!("expected while step");
    };
    assert!(matches!(body[0], Step::AssignStep { .. }));
}

#[test]
fn binary_expression_round_trips() {
    let plan = plan(vec![Step::AssignStep {
        target: "result".to_string(),
        value: Expression::BinaryExpression {
            operator: BinaryOperator::Modulo,
            left: Box::new(lit(10)),
            right: Box::new(lit(3)),
        },
    }]);
    let rt = roundtrip(&plan);
    let Step::AssignStep { value, .. } = &rt.steps[0] else {
        panic!("expected assign step");
    };
    assert!(matches!(value, Expression::BinaryExpression { .. }));
}

#[test]
fn concat_expression_round_trips() {
    let plan = plan(vec![Step::AssignStep {
        target: "message".to_string(),
        value: Expression::ConcatExpression {
            parts: vec![lit("Hello "), var("name")],
        },
    }]);
    let rt = roundtrip(&plan);
    let Step::AssignStep { value, .. } = &rt.steps[0] else {
        panic!("expected assign step");
    };
    assert!(matches!(value, Expression::ConcatExpression { .. }));
}

#[test]
fn literal_expression_in_while_condition_round_trips() {
    let plan = plan(vec![Step::WhileStep {
        condition: lit(42),
        body: vec![],
    }]);
    let rt = roundtrip(&plan);
    let Step::WhileStep { condition, .. } = &rt.steps[0] else {
        panic!("expected while step");
    };
    assert_eq!(condition, &lit(42));
}

#[test]
fn access_path_in_while_condition_round_trips() {
    let plan = plan(vec![Step::WhileStep {
        condition: var("flag"),
        body: vec![],
    }]);
    let rt = roundtrip(&plan);
    let Step::WhileStep { condition, .. } = &rt.steps[0] else {
        panic!("expected while step");
    };
    assert_eq!(condition, &var("flag"));
}

#[test]
fn comparison_expression_in_branch_condition_round_trips() {
    let plan = plan(vec![Step::SwitchStep {
        branches: vec![ConditionalBranch {
            kind: BranchKind::If,
            condition: Some(Expression::Comparison {
                operator: ComparisonOperator::Equal,
                left: Box::new(var("x")),
                right: Box::new(lit(1)),
            }),
            steps: vec![],
        }],
    }]);
    let rt = roundtrip(&plan);
    let Step::SwitchStep { branches } = &rt.steps[0] else {
        panic!("expected switch step");
    };
    assert!(matches!(
        branches[0].condition,
        Some(Expression::Comparison { .. })
    ));
}

#[test]
fn not_expression_round_trips() {
    let plan = plan(vec![Step::WhileStep {
        condition: Expression::Not {
            operand: Box::new(lit(false)),
        },
        body: vec![],
    }]);
    let rt = roundtrip(&plan);
    let Step::WhileStep { condition, .. } = &rt.steps[0] else {
        panic!("expected while step");
    };
    assert!(matches!(condition, Expression::Not { .. }));
}

#[test]
fn boolean_expression_round_trips() {
    let plan = plan(vec![Step::WhileStep {
        condition: Expression::Boolean {
            operator: BooleanOperator::And,
            operands: vec![lit(true), var("active")],
        },
        body: vec![],
    }]);
    let rt = roundtrip(&plan);
    let Step::WhileStep { condition, .. } = &rt.steps[0] else {
        panic!("expected while step");
    };
    assert!(matches!(condition, Expression::Boolean { .. }));
}

#[test]
fn prefillable_inputs_found_after_round_trip() {
    let plan = plan(vec![
        Step::UserInteractionStep {
            prompt: "Provide doc URL:".to_string(),
            output_variable: Some("doc_url".to_string()),
        },
        Step::UserInteractionStep {
            prompt: "Slack channel:".to_string(),
            output_variable: Some("channel".to_string()),
        },
        Step::ToolStep {
            tool_id: "fetch".to_string(),
            arguments: vec![var("doc_url")],
            output_variable: Some("content".to_string()),
        },
    ]);
    let rt = roundtrip(&plan);
    let prefillable = rt.prefillable_inputs();
    assert_eq!(prefillable.len(), 2);
    assert_eq!(prefillable[0].variable_name, "doc_url");
    assert_eq!(prefillable[1].variable_name, "channel");
}

#[test]
fn no_prefillable_inputs_when_all_nested() {
    let plan = plan(vec![Step::SwitchStep {
        branches: vec![ConditionalBranch {
            kind: BranchKind::If,
            condition: Some(lit(true)),
            steps: vec![Step::UserInteractionStep {
                prompt: "Nested?".to_string(),
                output_variable: Some("nested".to_string()),
            }],
        }],
    }]);
    let rt = roundtrip(&plan);
    assert_eq!(rt.prefillable_inputs(), Vec::<PrefillableInput>::new());
}

#[test]
fn mixed_plan_prefillable_inputs_correct_after_round_trip() {
    let plan = plan(vec![
        Step::UserInteractionStep {
            prompt: "Top-level question?".to_string(),
            output_variable: Some("top".to_string()),
        },
        Step::ToolStep {
            tool_id: "compute".to_string(),
            arguments: vec![],
            output_variable: Some("result".to_string()),
        },
        Step::SwitchStep {
            branches: vec![ConditionalBranch {
                kind: BranchKind::Else,
                condition: None,
                steps: vec![Step::UserInteractionStep {
                    prompt: "Nested?".to_string(),
                    output_variable: Some("nested".to_string()),
                }],
            }],
        },
        Step::UserInteractionStep {
            prompt: String::new(),
            output_variable: Some("empty_prompt".to_string()),
        },
        Step::UserInteractionStep {
            prompt: "No output".to_string(),
            output_variable: None,
        },
    ]);
    let rt = roundtrip(&plan);
    let prefillable = rt.prefillable_inputs();
    assert_eq!(prefillable.len(), 1);
    assert_eq!(prefillable[0].variable_name, "top");
}

#[test]
fn access_path_with_accessors_round_trips() {
    let plan = plan(vec![Step::ToolStep {
        tool_id: "use".to_string(),
        arguments: vec![Expression::AccessPath {
            variable_name: "data".to_string(),
            accessors: vec![
                Accessor::Key("items".to_string()),
                Accessor::Index(0),
                Accessor::Attribute("name".to_string()),
            ],
        }],
        output_variable: None,
    }]);
    let rt = roundtrip(&plan);
    let Step::ToolStep { arguments, .. } = &rt.steps[0] else {
        panic!("expected tool step");
    };
    let Expression::AccessPath {
        variable_name,
        accessors,
    } = &arguments[0]
    else {
        panic!("expected access path");
    };
    assert_eq!(variable_name, "data");
    assert_eq!(accessors.len(), 3);
    assert_eq!(accessors[0], Accessor::Key("items".to_string()));
    assert_eq!(accessors[1], Accessor::Index(0));
    assert_eq!(accessors[2], Accessor::Attribute("name".to_string()));
}

#[test]
fn access_path_in_condition_round_trips() {
    let plan = plan(vec![Step::WhileStep {
        condition: Expression::Comparison {
            operator: ComparisonOperator::GreaterThan,
            left: Box::new(Expression::AccessPath {
                variable_name: "result".to_string(),
                accessors: vec![Accessor::Attribute("count".to_string())],
            }),
            right: Box::new(lit(0)),
        },
        body: vec![],
    }]);
    let rt = roundtrip(&plan);
    let Step::WhileStep { condition, .. } = &rt.steps[0] else {
        panic!("expected while step");
    };
    let Expression::Comparison { left, .. } = condition else {
        panic!("expected comparison");
    };
    let Expression::AccessPath { accessors, .. } = left.as_ref() else {
        panic!("expected access path");
    };
    assert_eq!(accessors.len(), 1);
    assert_eq!(accessors[0], Accessor::Attribute("count".to_string()));
}

#[test]
fn prefillable_inputs_returns_only_top_level_user_interactions_with_output_and_prompt() {
    let plan = plan(vec![
        Step::UserInteractionStep {
            prompt: "What is your name?".to_string(),
            output_variable: Some("name".to_string()),
        },
        Step::UserInteractionStep {
            prompt: String::new(),
            output_variable: Some("empty_prompt".to_string()),
        },
        Step::UserInteractionStep {
            prompt: "Acknowledge".to_string(),
            output_variable: None,
        },
        Step::SwitchStep {
            branches: vec![ConditionalBranch {
                kind: BranchKind::If,
                condition: Some(lit(true)),
                steps: vec![Step::UserInteractionStep {
                    prompt: "Nested".to_string(),
                    output_variable: Some("nested".to_string()),
                }],
            }],
        },
    ]);

    assert_eq!(
        plan.prefillable_inputs(),
        vec![PrefillableInput {
            variable_name: "name".to_string(),
            prompt: "What is your name?".to_string(),
        }]
    );
}

#[test]
fn collect_tool_ids_collects_unique_ids_recursively() {
    let plan = plan(vec![
        Step::ToolStep {
            tool_id: "top".to_string(),
            arguments: vec![],
            output_variable: None,
        },
        Step::SwitchStep {
            branches: vec![
                ConditionalBranch {
                    kind: BranchKind::If,
                    condition: Some(lit(true)),
                    steps: vec![
                        Step::ToolStep {
                            tool_id: "shared".to_string(),
                            arguments: vec![],
                            output_variable: None,
                        },
                        Step::ToolStep {
                            tool_id: "branch".to_string(),
                            arguments: vec![],
                            output_variable: None,
                        },
                    ],
                },
                ConditionalBranch {
                    kind: BranchKind::Else,
                    condition: None,
                    steps: vec![Step::ToolStep {
                        tool_id: "shared".to_string(),
                        arguments: vec![],
                        output_variable: None,
                    }],
                },
            ],
        },
        Step::LoopStep {
            iterable_variable: "items".to_string(),
            item_variable: "item".to_string(),
            body: vec![
                Step::ToolStep {
                    tool_id: "loop".to_string(),
                    arguments: vec![],
                    output_variable: None,
                },
                Step::ToolStep {
                    tool_id: "shared".to_string(),
                    arguments: vec![],
                    output_variable: None,
                },
            ],
        },
        Step::WhileStep {
            condition: lit(true),
            body: vec![Step::ToolStep {
                tool_id: "while".to_string(),
                arguments: vec![],
                output_variable: None,
            }],
        },
    ]);

    assert_eq!(
        plan.collect_tool_ids(),
        vec![
            "branch".to_string(),
            "loop".to_string(),
            "shared".to_string(),
            "top".to_string(),
            "while".to_string(),
        ]
    );
}

#[test]
fn roundtripped_plan_still_validates_for_prefillable_regression_shape() {
    let plan = plan(vec![
        Step::UserInteractionStep {
            prompt: "Provide doc URL:".to_string(),
            output_variable: Some("doc_url".to_string()),
        },
        Step::ToolStep {
            tool_id: "fetch".to_string(),
            arguments: vec![var("doc_url")],
            output_variable: Some("content".to_string()),
        },
    ]);
    let rt = roundtrip(&plan);
    let diagnostics = validate_plan(&rt, &[tool("fetch")]);
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic.severity != DiagnosticSeverity::Error)
    );
}
