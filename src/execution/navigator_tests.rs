use super::navigator::*;
use crate::execution::state::{InstructionPointer, InternalState};
use crate::plan::{
    BranchKind, ComparisonOperator, ConditionalBranch, Expression, LiteralValue, Step,
};
use crate::test_helpers::to_literal;
use serde_json::json;
use std::collections::HashMap;

fn literal(value: serde_json::Value) -> LiteralValue {
    to_literal(value)
}

fn tool(tool_id: &str) -> Step {
    Step::ToolStep {
        tool_id: tool_id.to_string(),
        arguments: Vec::new(),
        output_variable: None,
    }
}

fn loop_step(body: Option<Vec<Step>>) -> Step {
    Step::LoopStep {
        iterable_variable: "items".to_string(),
        item_variable: "item".to_string(),
        body: body.unwrap_or_else(|| vec![tool("body_step")]),
    }
}

fn while_step(body: Option<Vec<Step>>) -> Step {
    Step::WhileStep {
        condition: Expression::Literal {
            value: literal(json!(true)),
        },
        body: body.unwrap_or_else(|| vec![tool("while_step")]),
    }
}

fn switch_step(branches: Option<Vec<ConditionalBranch>>) -> Step {
    Step::SwitchStep {
        branches: branches.unwrap_or_else(|| {
            vec![
                ConditionalBranch {
                    kind: BranchKind::If,
                    condition: Some(Expression::Literal {
                        value: literal(json!(true)),
                    }),
                    steps: vec![tool("if_step")],
                },
                ConditionalBranch {
                    kind: BranchKind::Else,
                    condition: None,
                    steps: vec![tool("else_step")],
                },
            ]
        }),
    }
}

fn fresh_state() -> InternalState {
    InternalState::default()
}

#[test]
fn resolve_step_first_step_flat_plan() {
    let steps = vec![tool("a"), tool("b"), tool("c")];
    let ip = InstructionPointer { path: vec![0] };
    let result = resolve_step(&steps, &ip, &fresh_state()).expect("step should resolve");
    assert!(matches!(result.0, Some(step) if step == &steps[0]));
    assert_eq!(result.1, steps.as_slice());
}

#[test]
fn resolve_step_last_step_flat_plan() {
    let steps = vec![tool("a"), tool("b")];
    let ip = InstructionPointer { path: vec![1] };
    let result = resolve_step(&steps, &ip, &fresh_state()).expect("step should resolve");
    assert!(matches!(result.0, Some(step) if step == &steps[1]));
}

#[test]
fn resolve_step_past_end_returns_none() {
    let steps = vec![tool("a")];
    let ip = InstructionPointer { path: vec![5] };
    assert!(resolve_step(&steps, &ip, &fresh_state()).is_none());
}

#[test]
fn resolve_step_empty_plan_past_end() {
    assert!(resolve_step(&[], &InstructionPointer { path: vec![0] }, &fresh_state()).is_none());
}

#[test]
fn resolve_step_first_body_step_in_loop() {
    let body = vec![tool("first"), tool("second")];
    let steps = vec![loop_step(Some(body.clone()))];
    let ip = InstructionPointer { path: vec![0, 0] };
    let result = resolve_step(&steps, &ip, &fresh_state()).expect("step should resolve");
    assert!(matches!(result.0, Some(step) if step == &body[0]));
}

#[test]
fn resolve_step_second_body_step_in_loop() {
    let body = vec![tool("first"), tool("second")];
    let steps = vec![loop_step(Some(body.clone()))];
    let ip = InstructionPointer { path: vec![0, 1] };
    let result = resolve_step(&steps, &ip, &fresh_state()).expect("step should resolve");
    assert!(matches!(result.0, Some(step) if step == &body[1]));
}

#[test]
fn resolve_step_past_end_of_loop_body_returns_none_step() {
    let steps = vec![loop_step(Some(vec![tool("only")]))];
    let ip = InstructionPointer { path: vec![0, 5] };
    let result = resolve_step(&steps, &ip, &fresh_state()).expect("resolution should exist");
    assert!(result.0.is_none());
}

#[test]
fn resolve_step_resolves_selected_switch_branch_step() {
    let if_step = tool("if_body");
    let switch = switch_step(Some(vec![ConditionalBranch {
        kind: BranchKind::If,
        condition: Some(Expression::Literal {
            value: literal(json!(true)),
        }),
        steps: vec![if_step.clone()],
    }]));
    let steps = vec![switch];
    let ip = InstructionPointer { path: vec![0, 0] };
    let mut state = fresh_state();
    state.branches.insert(vec![0], 0);

    let result = resolve_step(&steps, &ip, &state).expect("step should resolve");
    assert!(matches!(result.0, Some(step) if step == &if_step));
}

#[test]
fn resolve_step_with_no_selected_switch_branch_returns_empty() {
    let steps = vec![switch_step(None)];
    let ip = InstructionPointer { path: vec![0, 0] };
    let result = resolve_step(&steps, &ip, &fresh_state()).expect("resolution should exist");
    assert!(result.0.is_none());
}

#[test]
fn step_at_path_top_level() {
    let steps = vec![tool("a"), tool("b")];
    assert_eq!(step_at_path(&steps, &[1], &fresh_state()), Some(&steps[1]));
}

#[test]
fn step_at_path_nested_loop() {
    let body = vec![tool("inner")];
    let steps = vec![loop_step(Some(body.clone()))];
    assert_eq!(
        step_at_path(&steps, &[0, 0], &fresh_state()),
        Some(&body[0])
    );
}

#[test]
fn step_at_path_out_of_range_returns_none() {
    let steps = vec![tool("only")];
    assert!(step_at_path(&steps, &[99], &fresh_state()).is_none());
}

#[test]
fn get_child_steps_loop_returns_body() {
    let body = vec![tool("a"), tool("b")];
    let step = loop_step(Some(body.clone()));
    assert_eq!(
        get_child_steps(&step, &[0], &fresh_state()),
        body.as_slice()
    );
}

#[test]
fn get_child_steps_while_returns_body() {
    let body = vec![tool("x")];
    let step = while_step(Some(body.clone()));
    assert_eq!(
        get_child_steps(&step, &[0], &fresh_state()),
        body.as_slice()
    );
}

#[test]
fn get_child_steps_switch_selected_branch() {
    let if_steps = vec![tool("if_body")];
    let step = switch_step(Some(vec![
        ConditionalBranch {
            kind: BranchKind::If,
            condition: Some(Expression::Literal {
                value: literal(json!(true)),
            }),
            steps: if_steps.clone(),
        },
        ConditionalBranch {
            kind: BranchKind::Else,
            condition: None,
            steps: vec![tool("else_body")],
        },
    ]));
    let mut state = fresh_state();
    state.branches.insert(vec![0], 0);
    assert_eq!(get_child_steps(&step, &[0], &state), if_steps.as_slice());
}

#[test]
fn get_child_steps_switch_no_selected_branch_returns_empty() {
    let step = switch_step(None);
    assert!(get_child_steps(&step, &[0], &fresh_state()).is_empty());
}

#[test]
fn get_child_steps_switch_out_of_range_index_returns_empty() {
    let step = switch_step(None);
    let mut state = fresh_state();
    state.branches.insert(vec![0], 99);
    assert!(get_child_steps(&step, &[0], &state).is_empty());
}

#[test]
#[should_panic(expected = "IP descends into non-container step")]
fn get_child_steps_non_container_raises() {
    let step = tool("t");
    let _ = get_child_steps(&step, &[0], &fresh_state());
}

#[test]
fn on_scope_exit_loop_step_returns_to_parent() {
    let loop_step = loop_step(None);
    let mut state = fresh_state();
    let mut ip = InstructionPointer { path: vec![0, 5] };
    on_scope_exit(Some(&loop_step), &mut state, &mut ip, &[0]);
    assert_eq!(ip.path, vec![0]);
}

#[test]
fn on_scope_exit_while_step_returns_to_parent() {
    let while_step = while_step(None);
    let mut state = fresh_state();
    let mut ip = InstructionPointer { path: vec![1, 3] };
    on_scope_exit(Some(&while_step), &mut state, &mut ip, &[1]);
    assert_eq!(ip.path, vec![1]);
}

#[test]
fn on_scope_exit_switch_step_cleans_branch_and_ascends() {
    let switch = switch_step(None);
    let mut state = fresh_state();
    state.branches.insert(vec![2], 0);
    let mut ip = InstructionPointer { path: vec![2, 1] };
    on_scope_exit(Some(&switch), &mut state, &mut ip, &[2]);
    assert!(!state.branches.contains_key(&vec![2]));
    assert_eq!(ip.path, vec![3]);
}

#[test]
fn on_scope_exit_none_parent_ascends() {
    let mut ip = InstructionPointer { path: vec![0, 2] };
    let mut state = fresh_state();
    on_scope_exit(None, &mut state, &mut ip, &[0]);
    assert_eq!(ip.path, vec![1]);
}

#[test]
fn select_branch_first_branch_matches() {
    let step = switch_step(Some(vec![
        ConditionalBranch {
            kind: BranchKind::If,
            condition: Some(Expression::Literal {
                value: literal(json!(true)),
            }),
            steps: vec![tool("t")],
        },
        ConditionalBranch {
            kind: BranchKind::Else,
            condition: None,
            steps: vec![tool("u")],
        },
    ]));
    assert_eq!(select_branch(&step, &HashMap::new()), Some(0));
}

#[test]
fn select_branch_second_branch_matches_when_first_false() {
    let step = switch_step(Some(vec![
        ConditionalBranch {
            kind: BranchKind::If,
            condition: Some(Expression::Literal {
                value: literal(json!(false)),
            }),
            steps: vec![tool("t")],
        },
        ConditionalBranch {
            kind: BranchKind::Else,
            condition: None,
            steps: vec![tool("u")],
        },
    ]));
    assert_eq!(select_branch(&step, &HashMap::new()), Some(1));
}

#[test]
fn select_branch_else_branch_always_matches() {
    let step = switch_step(Some(vec![ConditionalBranch {
        kind: BranchKind::Else,
        condition: None,
        steps: vec![tool("t")],
    }]));
    assert_eq!(select_branch(&step, &HashMap::new()), Some(0));
}

#[test]
fn select_branch_no_matching_branch_returns_none() {
    let step = switch_step(Some(vec![ConditionalBranch {
        kind: BranchKind::If,
        condition: Some(Expression::Literal {
            value: literal(json!(false)),
        }),
        steps: vec![tool("t")],
    }]));
    assert_eq!(select_branch(&step, &HashMap::new()), None);
}

#[test]
fn select_branch_uses_variable_scope() {
    let step = switch_step(Some(vec![
        ConditionalBranch {
            kind: BranchKind::If,
            condition: Some(Expression::Comparison {
                operator: ComparisonOperator::GreaterThan,
                left: Box::new(Expression::AccessPath {
                    variable_name: "score".to_string(),
                    accessors: Vec::new(),
                }),
                right: Box::new(Expression::Literal {
                    value: literal(json!(50)),
                }),
            }),
            steps: vec![tool("t")],
        },
        ConditionalBranch {
            kind: BranchKind::Else,
            condition: None,
            steps: vec![tool("u")],
        },
    ]));

    let high = HashMap::from([(String::from("score"), json!(75))]);
    let low = HashMap::from([(String::from("score"), json!(25))]);
    assert_eq!(select_branch(&step, &high), Some(0));
    assert_eq!(select_branch(&step, &low), Some(1));
}
