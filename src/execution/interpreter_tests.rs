use crate::error::{ReadyError, Result};
use crate::execution::interpreter::{
    PlanInterpreter, handle_assign_step, handle_loop_step, handle_switch_step,
    handle_user_interaction_step, handle_while_step,
};
use crate::execution::state::{ExecutionState, ExecutionStatus, InstructionPointer, InternalState};
use crate::plan::{
    AbstractPlan, BranchKind, ComparisonOperator, ConditionalBranch, Expression, Step,
};
use crate::test_helpers::{HandlerToolsModule, to_literal};
use crate::tools::models::{
    ToolArgumentDescription, ToolCall, ToolDescription, ToolResult, ToolReturnDescription,
};
use crate::tools::registry::InMemoryToolRegistry;
use crate::tools::traits::ToolsModule;
use serde_json::Value;
use serde_json::json;
use std::pin::Pin;
use std::sync::Arc;

fn plan(steps: Vec<Step>) -> AbstractPlan {
    AbstractPlan {
        name: "test_plan".to_string(),
        description: String::new(),
        steps,
        code: String::new(),
    }
}

fn expr(value: Value) -> Expression {
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

fn assign(var: &str, value: Value) -> Step {
    Step::AssignStep {
        target: var.to_string(),
        value: expr(value),
    }
}

fn assign_expr(var: &str, value: Expression) -> Step {
    Step::AssignStep {
        target: var.to_string(),
        value,
    }
}

fn assign_ref(var: &str, source_var: &str) -> Step {
    assign_expr(var, access(source_var))
}

fn join_str(var: &str, parts: Vec<Expression>) -> Step {
    assign_expr(var, Expression::ConcatExpression { parts })
}

fn binary_assign(
    var: &str,
    operator: crate::plan::BinaryOperator,
    left: Value,
    right: Value,
) -> Step {
    assign_expr(
        var,
        Expression::BinaryExpression {
            operator,
            left: Box::new(expr(left)),
            right: Box::new(expr(right)),
        },
    )
}

fn tool_call(tool_id: &str, args: Vec<Expression>, output_variable: Option<&str>) -> Step {
    Step::ToolStep {
        tool_id: tool_id.to_string(),
        arguments: args,
        output_variable: output_variable.map(str::to_string),
    }
}

fn user_interaction(prompt: &str, output_variable: &str) -> Step {
    Step::UserInteractionStep {
        prompt: prompt.to_string(),
        output_variable: Some(output_variable.to_string()),
    }
}

fn tool_description(tool_id: &str, arguments: &[&str]) -> ToolDescription {
    ToolDescription {
        id: tool_id.to_string(),
        description: format!("Test tool: {tool_id}"),
        arguments: arguments
            .iter()
            .map(|name| ToolArgumentDescription {
                name: (*name).to_string(),
                description: String::new(),
                type_name: "Any".to_string(),
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

fn registry_with_handler(
    tools: Vec<ToolDescription>,
    handler: impl Fn(&str, Vec<Value>) -> Result<ToolResult> + Send + Sync + 'static,
) -> InMemoryToolRegistry {
    let mut registry = InMemoryToolRegistry::new();
    registry
        .register_module(Box::new(HandlerToolsModule::new(tools, handler)))
        .unwrap();
    registry
}

fn interpreter_for_plan(
    registry: Arc<InMemoryToolRegistry>,
    plan: &AbstractPlan,
) -> PlanInterpreter {
    PlanInterpreter::new(registry, plan.clone())
}

async fn execute_plan(
    registry: Arc<InMemoryToolRegistry>,
    plan: &AbstractPlan,
) -> std::result::Result<ExecutionState, ReadyError> {
    let interpreter = interpreter_for_plan(registry, plan);
    let mut state = ExecutionState::default();
    match interpreter.execute(&mut state).await {
        Ok(()) => Ok(state),
        Err(error) => Err(error),
    }
}

#[tokio::test]
async fn empty_plan_completes_with_empty_state() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let state = execute_plan(registry, &plan(vec![]))
        .await
        .expect("plan should execute");
    assert_eq!(state.status, ExecutionStatus::Completed);
    assert!(state.error.is_none());
    assert!(state.interpreter_state.variables.is_empty());
}

#[tokio::test]
async fn single_assignment_sets_variable_and_completes() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![assign("x", json!("hello"))]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(state.status, ExecutionStatus::Completed);
    assert_eq!(
        state.interpreter_state.variables.get("x"),
        Some(&json!("hello"))
    );
}

#[tokio::test]
async fn multiple_assignments_set_all_variables() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![
        assign("a", json!("alpha")),
        assign("b", json!(42)),
        assign("c", json!(true)),
    ]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(
        state.interpreter_state.variables.get("a"),
        Some(&json!("alpha"))
    );
    assert_eq!(state.interpreter_state.variables.get("b"), Some(&json!(42)));
    assert_eq!(
        state.interpreter_state.variables.get("c"),
        Some(&json!(true))
    );
}

#[tokio::test]
async fn string_join_concatenates_literals_and_variables() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![
        assign("name", json!("World")),
        join_str(
            "greeting",
            vec![expr(json!("Hello, ")), access("name"), expr(json!("!"))],
        ),
    ]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(
        state.interpreter_state.variables.get("greeting"),
        Some(&json!("Hello, World!"))
    );
}

#[tokio::test]
async fn string_join_converts_non_strings_to_string() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![
        assign("count", json!(5)),
        join_str("msg", vec![expr(json!("Items: ")), access("count")]),
    ]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(
        state.interpreter_state.variables.get("msg"),
        Some(&json!("Items: 5"))
    );
}

#[tokio::test]
async fn builtin_arithmetic_assignments_execute_through_expression_engine() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![
        binary_assign(
            "mod",
            crate::plan::BinaryOperator::Modulo,
            json!(10),
            json!(3),
        ),
        binary_assign(
            "sub",
            crate::plan::BinaryOperator::Subtract,
            json!(10),
            json!(3),
        ),
        binary_assign(
            "mul",
            crate::plan::BinaryOperator::Multiply,
            json!(6),
            json!(7),
        ),
        binary_assign(
            "floordiv",
            crate::plan::BinaryOperator::FloorDivide,
            json!(10),
            json!(3),
        ),
    ]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(
        state.interpreter_state.variables.get("mod"),
        Some(&json!(1))
    );
    assert_eq!(
        state.interpreter_state.variables.get("sub"),
        Some(&json!(7))
    );
    assert_eq!(
        state.interpreter_state.variables.get("mul"),
        Some(&json!(42))
    );
    assert_eq!(
        state.interpreter_state.variables.get("floordiv"),
        Some(&json!(3))
    );
}

#[test]
fn handle_assign_step_sets_value_without_full_interpreter() {
    let mut state = ExecutionState::default();
    let mut ip = InstructionPointer { path: vec![0] };

    handle_assign_step("x", &expr(json!(5)), &mut ip, &mut state).expect("assign should work");

    assert_eq!(state.interpreter_state.variables.get("x"), Some(&json!(5)));
    assert_eq!(ip.path, vec![1]);
}

#[test]
fn handle_switch_step_descends_into_selected_branch_without_run_loop() {
    let mut ip = InstructionPointer { path: vec![0] };
    let mut internal_state = InternalState::default();
    let step = Step::SwitchStep {
        branches: vec![ConditionalBranch {
            kind: BranchKind::If,
            condition: Some(expr(json!(true))),
            steps: vec![assign("x", json!(1))],
        }],
    };

    let Step::SwitchStep { branches } = &step else {
        panic!("expected switch");
    };
    handle_switch_step(
        &step,
        branches,
        &mut ip,
        &std::collections::HashMap::new(),
        &mut internal_state,
    )
    .expect("switch should work");

    assert_eq!(ip.path, vec![0, 0]);
    assert_eq!(internal_state.branches.get(&vec![0]), Some(&0));
}

#[test]
fn handle_loop_step_initializes_iteration_state_without_execute() {
    let mut ip = InstructionPointer { path: vec![2] };
    let mut internal_state = InternalState::default();
    let mut variables = std::collections::HashMap::from([("items".to_string(), json!(["a", "b"]))]);

    handle_loop_step(
        "items",
        "item",
        &mut ip,
        Some(0),
        &mut variables,
        &mut internal_state,
    )
    .expect("loop should work");

    assert_eq!(variables.get("item"), Some(&json!("a")));
    assert_eq!(ip.path, vec![2, 0]);
    assert_eq!(internal_state.loops.get(&vec![2]).map(|s| s.index), Some(0));
}

#[test]
fn handle_while_step_stops_after_iteration_limit_without_run_loop() {
    let mut ip = InstructionPointer { path: vec![1] };
    let mut internal_state = InternalState::default();
    internal_state.whiles.insert(
        vec![1],
        crate::execution::state::WhileState { iterations: 1 },
    );

    let error = handle_while_step(
        &expr(json!(true)),
        &mut ip,
        Some(3),
        &std::collections::HashMap::new(),
        &mut internal_state,
        1,
    )
    .expect_err("while should hit limit");

    assert!(
        matches!(error, ReadyError::Execution { message, .. } if message.contains("exceeded maximum iterations"))
    );
}

#[test]
fn handle_user_interaction_step_suspends_without_full_plan_execution() {
    let mut ip = InstructionPointer { path: vec![4] };
    let mut interpreter_state = crate::execution::state::InterpreterState::default();

    let result = handle_user_interaction_step(
        "Enter your input:",
        &Some("answer".to_string()),
        &mut ip,
        &mut interpreter_state,
    )
    .expect("interaction should suspend");

    assert!(result.suspend);
    assert_eq!(result.suspend_reason.as_deref(), Some("Enter your input:"));
    assert_eq!(
        interpreter_state.pending_input_variable.as_deref(),
        Some("answer")
    );
    assert_eq!(ip.path, vec![4]);
}

#[tokio::test]
async fn tool_call_result_stored_in_output_variable() {
    let registry = Arc::new(registry_with_handler(
        vec![tool_description("add_numbers", &["a", "b"])],
        |tool_id, args| {
            assert_eq!(tool_id, "add_numbers");
            Ok(ToolResult::Success(json!(
                args[0].as_i64().unwrap() + args[1].as_i64().unwrap()
            )))
        },
    ));
    let plan = plan(vec![tool_call(
        "add_numbers",
        vec![expr(json!(3)), expr(json!(7))],
        Some("sum"),
    )]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(
        state.interpreter_state.variables.get("sum"),
        Some(&json!(10))
    );
}

#[tokio::test]
async fn tool_call_with_variable_arguments() {
    let registry = Arc::new(registry_with_handler(
        vec![tool_description("add_numbers", &["a", "b"])],
        |_tool_id, args| {
            Ok(ToolResult::Success(json!(
                args[0].as_i64().unwrap() + args[1].as_i64().unwrap()
            )))
        },
    ));
    let plan = plan(vec![
        assign("a", json!(10)),
        assign("b", json!(20)),
        tool_call("add_numbers", vec![access("a"), access("b")], Some("sum")),
    ]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(
        state.interpreter_state.variables.get("sum"),
        Some(&json!(30))
    );
}

#[tokio::test]
async fn tool_call_without_output_variable_completes() {
    let registry = Arc::new(registry_with_handler(
        vec![tool_description("add_numbers", &["a", "b"])],
        |_tool_id, _args| Ok(ToolResult::Success(json!(3))),
    ));
    let plan = plan(vec![tool_call(
        "add_numbers",
        vec![expr(json!(1)), expr(json!(2))],
        None,
    )]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(state.status, ExecutionStatus::Completed);
}

#[tokio::test]
async fn switch_if_true_executes_if_branch() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![
        assign("x", json!(10)),
        Step::SwitchStep {
            branches: vec![
                ConditionalBranch {
                    kind: BranchKind::If,
                    condition: Some(Expression::Comparison {
                        operator: ComparisonOperator::GreaterThan,
                        left: Box::new(access("x")),
                        right: Box::new(expr(json!(5))),
                    }),
                    steps: vec![assign("result", json!("big"))],
                },
                ConditionalBranch {
                    kind: BranchKind::Else,
                    condition: None,
                    steps: vec![assign("result", json!("small"))],
                },
            ],
        },
    ]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(
        state.interpreter_state.variables.get("result"),
        Some(&json!("big"))
    );
}

#[tokio::test]
async fn switch_if_false_executes_else_branch() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![
        assign("x", json!(2)),
        Step::SwitchStep {
            branches: vec![
                ConditionalBranch {
                    kind: BranchKind::If,
                    condition: Some(Expression::Comparison {
                        operator: ComparisonOperator::GreaterThan,
                        left: Box::new(access("x")),
                        right: Box::new(expr(json!(5))),
                    }),
                    steps: vec![assign("result", json!("big"))],
                },
                ConditionalBranch {
                    kind: BranchKind::Else,
                    condition: None,
                    steps: vec![assign("result", json!("small"))],
                },
            ],
        },
    ]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(
        state.interpreter_state.variables.get("result"),
        Some(&json!("small"))
    );
}

#[tokio::test]
async fn switch_without_else_skips_when_condition_false() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![
        assign("x", json!(2)),
        assign("result", json!("unchanged")),
        Step::SwitchStep {
            branches: vec![ConditionalBranch {
                kind: BranchKind::If,
                condition: Some(Expression::Comparison {
                    operator: ComparisonOperator::GreaterThan,
                    left: Box::new(access("x")),
                    right: Box::new(expr(json!(5))),
                }),
                steps: vec![assign("result", json!("changed"))],
            }],
        },
    ]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(
        state.interpreter_state.variables.get("result"),
        Some(&json!("unchanged"))
    );
}

#[tokio::test]
async fn switch_with_elif_executes_elif_branch() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![
        assign("x", json!(4)),
        Step::SwitchStep {
            branches: vec![
                ConditionalBranch {
                    kind: BranchKind::If,
                    condition: Some(Expression::Comparison {
                        operator: ComparisonOperator::GreaterThan,
                        left: Box::new(access("x")),
                        right: Box::new(expr(json!(10))),
                    }),
                    steps: vec![assign("result", json!("very big"))],
                },
                ConditionalBranch {
                    kind: BranchKind::ElseIf,
                    condition: Some(Expression::Comparison {
                        operator: ComparisonOperator::GreaterThan,
                        left: Box::new(access("x")),
                        right: Box::new(expr(json!(3))),
                    }),
                    steps: vec![assign("result", json!("medium"))],
                },
                ConditionalBranch {
                    kind: BranchKind::Else,
                    condition: None,
                    steps: vec![assign("result", json!("small"))],
                },
            ],
        },
    ]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(
        state.interpreter_state.variables.get("result"),
        Some(&json!("medium"))
    );
}

#[tokio::test]
async fn switch_with_elif_falls_through_to_else() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![
        assign("x", json!(1)),
        Step::SwitchStep {
            branches: vec![
                ConditionalBranch {
                    kind: BranchKind::If,
                    condition: Some(Expression::Comparison {
                        operator: ComparisonOperator::GreaterThan,
                        left: Box::new(access("x")),
                        right: Box::new(expr(json!(10))),
                    }),
                    steps: vec![assign("result", json!("very big"))],
                },
                ConditionalBranch {
                    kind: BranchKind::ElseIf,
                    condition: Some(Expression::Comparison {
                        operator: ComparisonOperator::GreaterThan,
                        left: Box::new(access("x")),
                        right: Box::new(expr(json!(3))),
                    }),
                    steps: vec![assign("result", json!("medium"))],
                },
                ConditionalBranch {
                    kind: BranchKind::Else,
                    condition: None,
                    steps: vec![assign("result", json!("small"))],
                },
            ],
        },
    ]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(
        state.interpreter_state.variables.get("result"),
        Some(&json!("small"))
    );
}

#[tokio::test]
async fn for_loop_iterates_over_list() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![
        assign("items", json!([1, 2, 3])),
        assign("last", Value::Null),
        Step::LoopStep {
            iterable_variable: "items".to_string(),
            item_variable: "item".to_string(),
            body: vec![assign_ref("last", "item")],
        },
    ]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(
        state.interpreter_state.variables.get("last"),
        Some(&json!(3))
    );
}

#[tokio::test]
async fn for_loop_accumulates_with_tool() {
    let registry = Arc::new(registry_with_handler(
        vec![tool_description("add_numbers", &["a", "b"])],
        |_tool_id, args| {
            Ok(ToolResult::Success(json!(
                args[0].as_i64().unwrap() + args[1].as_i64().unwrap()
            )))
        },
    ));
    let plan = plan(vec![
        assign("numbers", json!([10, 20, 30])),
        assign("total", json!(0)),
        Step::LoopStep {
            iterable_variable: "numbers".to_string(),
            item_variable: "n".to_string(),
            body: vec![tool_call(
                "add_numbers",
                vec![access("total"), access("n")],
                Some("total"),
            )],
        },
    ]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(
        state.interpreter_state.variables.get("total"),
        Some(&json!(60))
    );
}

#[tokio::test]
async fn for_loop_with_empty_iterable_executes_no_body() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![
        assign("items", json!([])),
        assign("touched", json!(false)),
        Step::LoopStep {
            iterable_variable: "items".to_string(),
            item_variable: "item".to_string(),
            body: vec![assign("touched", json!(true))],
        },
    ]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(
        state.interpreter_state.variables.get("touched"),
        Some(&json!(false))
    );
}

#[tokio::test]
async fn nested_loop_inside_switch_branch() {
    let registry = Arc::new(registry_with_handler(
        vec![tool_description("add_numbers", &["a", "b"])],
        |_tool_id, args| {
            Ok(ToolResult::Success(json!(
                args[0].as_i64().unwrap() + args[1].as_i64().unwrap()
            )))
        },
    ));
    let plan = plan(vec![
        assign("has_items", json!(true)),
        assign("items", json!([10, 20])),
        assign("total", json!(0)),
        Step::SwitchStep {
            branches: vec![
                ConditionalBranch {
                    kind: BranchKind::If,
                    condition: Some(access("has_items")),
                    steps: vec![Step::LoopStep {
                        iterable_variable: "items".to_string(),
                        item_variable: "item".to_string(),
                        body: vec![tool_call(
                            "add_numbers",
                            vec![access("total"), access("item")],
                            Some("total"),
                        )],
                    }],
                },
                ConditionalBranch {
                    kind: BranchKind::Else,
                    condition: None,
                    steps: vec![assign("total", json!(-1))],
                },
            ],
        },
    ]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(
        state.interpreter_state.variables.get("total"),
        Some(&json!(30))
    );
}

#[tokio::test]
async fn user_interaction_suspends() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![user_interaction("What is your name?", "name")]);
    let interpreter = interpreter_for_plan(registry, &plan);
    let mut state = ExecutionState::default();
    interpreter
        .execute(&mut state)
        .await
        .expect("execution should suspend");
    assert_eq!(state.status, ExecutionStatus::Suspended);
    assert_eq!(
        state.suspension_reason,
        Some("What is your name?".to_string())
    );
}

#[tokio::test]
async fn provide_input_resumes_and_completes() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![user_interaction("Name?", "name")]);
    let interpreter = interpreter_for_plan(registry, &plan);
    let mut state = ExecutionState::default();
    interpreter
        .execute(&mut state)
        .await
        .expect("execution should suspend");
    interpreter
        .provide_input(&mut state, json!("Alice"))
        .await
        .expect("resume should succeed");
    assert_eq!(state.status, ExecutionStatus::Completed);
    assert_eq!(
        state.interpreter_state.variables.get("name"),
        Some(&json!("Alice"))
    );
}

#[tokio::test]
async fn user_interaction_mid_plan_preserves_previous_steps() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![
        assign("greeting", json!("Hello")),
        user_interaction("Name?", "name"),
        join_str(
            "msg",
            vec![
                access("greeting"),
                expr(json!(", ")),
                access("name"),
                expr(json!("!")),
            ],
        ),
    ]);
    let interpreter = interpreter_for_plan(registry, &plan);
    let mut state = ExecutionState::default();
    interpreter
        .execute(&mut state)
        .await
        .expect("execution should suspend");
    assert_eq!(state.status, ExecutionStatus::Suspended);
    assert_eq!(
        state.interpreter_state.variables.get("greeting"),
        Some(&json!("Hello"))
    );

    interpreter
        .provide_input(&mut state, json!("Bob"))
        .await
        .expect("resume should succeed");
    assert_eq!(state.status, ExecutionStatus::Completed);
    assert_eq!(
        state.interpreter_state.variables.get("msg"),
        Some(&json!("Hello, Bob!"))
    );
}

#[tokio::test]
async fn multiple_user_interactions_suspend_twice_and_complete() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![
        user_interaction("First name?", "first"),
        user_interaction("Last name?", "last"),
        join_str(
            "full",
            vec![access("first"), expr(json!(" ")), access("last")],
        ),
    ]);
    let interpreter = interpreter_for_plan(registry, &plan);
    let mut state = ExecutionState::default();

    interpreter
        .execute(&mut state)
        .await
        .expect("first suspension expected");
    assert_eq!(state.status, ExecutionStatus::Suspended);
    assert_eq!(state.suspension_reason, Some("First name?".to_string()));

    interpreter
        .provide_input(&mut state, json!("Alice"))
        .await
        .expect("second suspension expected");
    assert_eq!(state.status, ExecutionStatus::Suspended);
    assert_eq!(state.suspension_reason, Some("Last name?".to_string()));

    interpreter
        .provide_input(&mut state, json!("Smith"))
        .await
        .expect("plan should complete");
    assert_eq!(state.status, ExecutionStatus::Completed);
    assert_eq!(
        state.interpreter_state.variables.get("full"),
        Some(&json!("Alice Smith"))
    );
}

#[tokio::test]
async fn unknown_tool_fails_with_runtime_shape_error() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let unresolved_plan = AbstractPlan {
        name: "test_plan".to_string(),
        description: String::new(),
        code: String::new(),
        steps: vec![tool_call(
            "nonexistent_tool",
            vec![expr(json!(1)), expr(json!(2))],
            Some("r"),
        )],
    };
    let interpreter = PlanInterpreter::new(registry, unresolved_plan);
    let mut state = ExecutionState::default();
    let error = interpreter
        .execute(&mut state)
        .await
        .expect_err("execution should fail");
    assert_eq!(state.status, ExecutionStatus::Failed);
    assert!(state.error.is_some());
    assert_eq!(
        state.error.as_ref().map(|e| e.exception_type.clone()),
        Some(error.to_string())
    );
}

#[tokio::test]
async fn undefined_variable_in_tool_arguments_fails() {
    let registry = Arc::new(registry_with_handler(
        vec![tool_description("add_numbers", &["a", "b"])],
        |_tool_id, _args| Ok(ToolResult::Success(json!(0))),
    ));
    let plan = plan(vec![tool_call(
        "add_numbers",
        vec![access("undefined_var"), expr(json!(1))],
        Some("r"),
    )]);
    let mut state = ExecutionState::default();
    let interpreter = interpreter_for_plan(registry, &plan);
    let _ = interpreter.execute(&mut state).await;
    assert_eq!(state.status, ExecutionStatus::Failed);
}

#[tokio::test]
async fn while_step_counts_down_to_zero() {
    let registry = Arc::new(registry_with_handler(
        vec![tool_description("add_numbers", &["a", "b"])],
        |_tool_id, args| {
            Ok(ToolResult::Success(json!(
                args[0].as_i64().unwrap() + args[1].as_i64().unwrap()
            )))
        },
    ));
    let plan = plan(vec![
        assign("counter", json!(3)),
        Step::WhileStep {
            condition: Expression::Comparison {
                operator: ComparisonOperator::GreaterThan,
                left: Box::new(access("counter")),
                right: Box::new(expr(json!(0))),
            },
            body: vec![tool_call(
                "add_numbers",
                vec![access("counter"), expr(json!(-1))],
                Some("counter"),
            )],
        },
    ]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(state.status, ExecutionStatus::Completed);
    assert_eq!(
        state.interpreter_state.variables.get("counter"),
        Some(&json!(0))
    );
}

#[tokio::test]
async fn while_step_accumulates_total() {
    let registry = Arc::new(registry_with_handler(
        vec![tool_description("add_numbers", &["a", "b"])],
        |_tool_id, args| {
            Ok(ToolResult::Success(json!(
                args[0].as_i64().unwrap() + args[1].as_i64().unwrap()
            )))
        },
    ));
    let plan = plan(vec![
        assign("counter", json!(5)),
        assign("total", json!(0)),
        Step::WhileStep {
            condition: Expression::Comparison {
                operator: ComparisonOperator::GreaterThan,
                left: Box::new(access("counter")),
                right: Box::new(expr(json!(0))),
            },
            body: vec![
                tool_call(
                    "add_numbers",
                    vec![access("total"), access("counter")],
                    Some("total"),
                ),
                tool_call(
                    "add_numbers",
                    vec![access("counter"), expr(json!(-1))],
                    Some("counter"),
                ),
            ],
        },
    ]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(state.status, ExecutionStatus::Completed);
    assert_eq!(
        state.interpreter_state.variables.get("total"),
        Some(&json!(15))
    );
    assert_eq!(
        state.interpreter_state.variables.get("counter"),
        Some(&json!(0))
    );
}

#[tokio::test]
async fn while_step_false_initially_never_executes_body() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![
        assign("x", json!(0)),
        assign("touched", json!(false)),
        Step::WhileStep {
            condition: Expression::Comparison {
                operator: ComparisonOperator::GreaterThan,
                left: Box::new(access("x")),
                right: Box::new(expr(json!(0))),
            },
            body: vec![assign("touched", json!(true))],
        },
    ]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(state.status, ExecutionStatus::Completed);
    assert_eq!(
        state.interpreter_state.variables.get("touched"),
        Some(&json!(false))
    );
}

#[tokio::test]
async fn while_step_exceeds_max_iterations_fails() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![
        assign("flag", json!(true)),
        Step::WhileStep {
            condition: access("flag"),
            body: vec![assign("x", json!("still going"))],
        },
    ]);
    let interpreter = interpreter_for_plan(registry, &plan).with_max_while_iterations(5);
    let mut state = ExecutionState::default();
    let error = interpreter
        .execute(&mut state)
        .await
        .expect_err("execution should fail");
    assert_eq!(state.status, ExecutionStatus::Failed);
    assert!(
        error
            .to_string()
            .to_lowercase()
            .contains("maximum iterations")
    );
}

#[tokio::test]
async fn while_step_with_not_condition() {
    let registry = Arc::new(InMemoryToolRegistry::new());
    let plan = plan(vec![
        assign("done", json!(false)),
        assign("count", json!(0)),
        Step::WhileStep {
            condition: Expression::Not {
                operand: Box::new(access("done")),
            },
            body: vec![assign("count", json!(1)), assign("done", json!(true))],
        },
    ]);
    let state = execute_plan(registry, &plan)
        .await
        .expect("plan should execute");
    assert_eq!(state.status, ExecutionStatus::Completed);
    assert_eq!(
        state.interpreter_state.variables.get("count"),
        Some(&json!(1))
    );
    assert_eq!(
        state.interpreter_state.variables.get("done"),
        Some(&json!(true))
    );
}

struct SuspendingModule {
    execute_calls: Arc<std::sync::Mutex<Vec<(Option<Value>, Option<Value>)>>>,
    tools: Vec<ToolDescription>,
}

impl SuspendingModule {
    fn new(execute_calls: Arc<std::sync::Mutex<Vec<(Option<Value>, Option<Value>)>>>) -> Self {
        Self {
            execute_calls,
            tools: vec![tool_description("suspending_tool", &[])],
        }
    }
}

impl ToolsModule for SuspendingModule {
    fn tools(&self) -> &[ToolDescription] {
        &self.tools
    }

    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send + 'a>> {
        let continuation_state = call.continuation.as_ref().map(|c| c.state.clone());
        let resume_value = call
            .continuation
            .as_ref()
            .and_then(|c| c.resume_value.clone());
        self.execute_calls
            .lock()
            .expect("execute calls mutex poisoned")
            .push((continuation_state.clone(), resume_value.clone()));

        Box::pin(async move {
            if continuation_state.is_some() {
                return Ok(ToolResult::Success(resume_value.unwrap_or(Value::Null)));
            }
            Ok(ToolResult::Suspended(
                crate::tools::models::ToolSuspension {
                    reason: "need input".to_string(),
                    continuation_state: json!({"step":"waiting"}),
                },
            ))
        })
    }
}

struct MultiSuspendModule {
    resume_values: Arc<std::sync::Mutex<Vec<Value>>>,
    suspend_count: usize,
    tools: Vec<ToolDescription>,
}

impl MultiSuspendModule {
    fn new(resume_values: Arc<std::sync::Mutex<Vec<Value>>>, suspend_count: usize) -> Self {
        Self {
            resume_values,
            suspend_count,
            tools: vec![tool_description("multi_tool", &[])],
        }
    }
}

impl ToolsModule for MultiSuspendModule {
    fn tools(&self) -> &[ToolDescription] {
        &self.tools
    }

    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send + 'a>> {
        let continuation_state = call.continuation.as_ref().map(|c| c.state.clone());
        let resume_value = call
            .continuation
            .as_ref()
            .and_then(|c| c.resume_value.clone());
        let call_number = continuation_state
            .as_ref()
            .and_then(|state| state.get("call"))
            .and_then(Value::as_u64)
            .unwrap_or(1) as usize;

        if let Some(value) = resume_value {
            self.resume_values
                .lock()
                .expect("resume values mutex poisoned")
                .push(value);
        }

        let suspend_count = self.suspend_count;
        let resume_values = self.resume_values.clone();
        Box::pin(async move {
            if call_number < suspend_count {
                let next = call_number + 1;
                return Ok(ToolResult::Suspended(
                    crate::tools::models::ToolSuspension {
                        reason: format!("round {next}"),
                        continuation_state: json!({"call": next}),
                    },
                ));
            }
            Ok(ToolResult::Success(Value::Array(
                resume_values
                    .lock()
                    .expect("resume values mutex poisoned")
                    .clone(),
            )))
        })
    }
}

#[tokio::test]
async fn tool_suspension_round_trip_and_resume_path_work() {
    let execute_calls = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut registry = InMemoryToolRegistry::new();
    registry
        .register_module(Box::new(SuspendingModule::new(execute_calls.clone())))
        .unwrap();

    let plan = plan(vec![tool_call("suspending_tool", vec![], Some("result"))]);
    let interpreter = interpreter_for_plan(Arc::new(registry), &plan);
    let mut state = ExecutionState::default();

    interpreter
        .execute(&mut state)
        .await
        .expect("initial execution should suspend");
    assert_eq!(state.status, ExecutionStatus::Suspended);
    assert_eq!(
        state.interpreter_state.pending_tool_id.as_deref(),
        Some("suspending_tool")
    );
    assert_eq!(
        state.interpreter_state.pending_tool_state,
        Some(json!({"step":"waiting"}))
    );
    assert_eq!(state.suspension_reason.as_deref(), Some("need input"));

    let serialized = serde_json::to_string(&state).expect("state should serialize");
    let mut restored: ExecutionState =
        serde_json::from_str(&serialized).expect("state should deserialize");
    interpreter
        .provide_input(&mut restored, json!("the answer"))
        .await
        .expect("resume should complete");

    assert_eq!(restored.status, ExecutionStatus::Completed);
    assert_eq!(
        restored.interpreter_state.variables.get("result"),
        Some(&json!("the answer"))
    );
    assert_eq!(
        *execute_calls.lock().expect("execute calls mutex poisoned"),
        vec![
            (None, None),
            (Some(json!({"step":"waiting"})), Some(json!("the answer")))
        ]
    );
}

#[tokio::test]
async fn multiple_suspend_resume_cycles_collect_values() {
    let resume_values = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut registry = InMemoryToolRegistry::new();
    registry
        .register_module(Box::new(MultiSuspendModule::new(resume_values.clone(), 3)))
        .unwrap();

    let plan = plan(vec![tool_call("multi_tool", vec![], Some("result"))]);
    let interpreter = interpreter_for_plan(Arc::new(registry), &plan);
    let mut state = ExecutionState::default();

    interpreter
        .execute(&mut state)
        .await
        .expect("first suspension expected");
    assert_eq!(state.suspension_reason.as_deref(), Some("round 2"));

    interpreter
        .provide_input(&mut state, json!("value_1"))
        .await
        .expect("second suspension expected");
    assert_eq!(state.status, ExecutionStatus::Suspended);
    assert_eq!(state.suspension_reason.as_deref(), Some("round 3"));

    interpreter
        .provide_input(&mut state, json!("value_2"))
        .await
        .expect("completion expected");
    assert_eq!(state.status, ExecutionStatus::Completed);
    assert_eq!(
        state.interpreter_state.variables.get("result"),
        Some(&json!(["value_1", "value_2"]))
    );
    assert_eq!(
        *resume_values.lock().expect("resume values mutex poisoned"),
        vec![json!("value_1"), json!("value_2")]
    );
}
