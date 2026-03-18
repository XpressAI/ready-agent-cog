use super::*;
use serde_json::json;

use crate::plan::{
    Accessor, BinaryOperator, BooleanOperator, BranchKind, ComparisonOperator, ConditionalBranch,
    Expression, LiteralValue, Step, UnaryOperator,
};
use crate::test_helpers::to_literal;

fn parse(code: &str) -> AbstractPlan {
    parse_python_to_plan(code, "main").expect("plan should parse")
}

fn parse_named(code: &str, name: &str) -> AbstractPlan {
    parse_python_to_plan(code, name).expect("plan should parse")
}

fn wrap_in_main(body: &str) -> String {
    let indented = body
        .lines()
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("def main():\n{indented}")
}

fn branch_by_kind<'a>(
    branches: &'a [ConditionalBranch],
    kind: BranchKind,
) -> &'a ConditionalBranch {
    branches
        .iter()
        .find(|branch| branch.kind == kind)
        .unwrap_or_else(|| panic!("missing branch: {kind:?}"))
}

fn branch_labels(branches: &[ConditionalBranch]) -> Vec<&str> {
    branches
        .iter()
        .map(|branch| match branch.kind {
            BranchKind::If => "if",
            BranchKind::ElseIf => "elif",
            BranchKind::Else => "else",
        })
        .collect()
}

fn access_path(name: &str) -> Expression {
    Expression::AccessPath {
        variable_name: name.to_string(),
        accessors: Vec::new(),
    }
}

fn literal(value: serde_json::Value) -> Expression {
    Expression::Literal {
        value: to_literal(value),
    }
}

#[test]
fn string_literal_produces_assign_step() {
    let plan = parse(&wrap_in_main("x = \"hello\""));

    assert_eq!(plan.steps.len(), 1);
    assert_eq!(
        plan.steps[0],
        Step::AssignStep {
            target: "x".to_string(),
            value: literal(json!("hello")),
        }
    );
}

#[test]
fn plan_stores_original_code() {
    let code = wrap_in_main("x = \"hello\"");
    let plan = parse(&code);
    assert_eq!(plan.code, code);
}

#[test]
fn plan_name_is_main() {
    let plan = parse(&wrap_in_main("x = \"hello\""));
    assert_eq!(plan.name, "main");
}

#[test]
fn explicit_name_sets_plan_name() {
    let code = wrap_in_main("x = \"hello\"");
    let plan = parse_named(&code, "my_sop");
    assert_eq!(plan.name, "my_sop");
}

#[test]
fn default_description_is_empty_string() {
    let code = wrap_in_main("x = \"hello\"");
    let plan = parse(&code);
    assert_eq!(plan.description, "");
}

#[test]
fn variable_assignment_produces_access_path() {
    let plan = parse(&wrap_in_main("y = \"world\"\nx = y"));
    assert_eq!(
        plan.steps[1],
        Step::AssignStep {
            target: "x".to_string(),
            value: access_path("y"),
        }
    );
}

#[test]
fn tool_call_with_mixed_args_produces_tool_step() {
    let plan = parse(&wrap_in_main("result = some_tool(\"arg1\", var2)"));
    assert_eq!(plan.steps.len(), 1);

    let Step::ToolStep {
        tool_id,
        output_variable,
        arguments,
    } = &plan.steps[0]
    else {
        panic!("expected tool step");
    };

    assert_eq!(tool_id, "some_tool");
    assert_eq!(output_variable.as_deref(), Some("result"));
    assert_eq!(
        arguments,
        &vec![literal(json!("arg1")), access_path("var2")]
    );
}

#[test]
fn bare_call_produces_tool_step_with_no_output() {
    let plan = parse(&wrap_in_main("some_tool(\"arg1\")"));
    let Step::ToolStep {
        tool_id,
        output_variable,
        arguments,
    } = &plan.steps[0]
    else {
        panic!("expected tool step");
    };

    assert_eq!(tool_id, "some_tool");
    assert!(output_variable.is_none());
    assert_eq!(arguments[0], literal(json!("arg1")));
}

#[test]
fn fstring_produces_concat_expression_assignment() {
    let plan = parse(&wrap_in_main("msg = f\"Hello {name}, you are {age}\""));
    let Step::AssignStep { target, value } = &plan.steps[0] else {
        panic!("expected assign step");
    };

    assert_eq!(target, &"msg".to_string());

    let Expression::ConcatExpression { parts } = value else {
        panic!("expected concat expression");
    };

    assert!(parts.contains(&access_path("name")));
    assert!(parts.contains(&access_path("age")));
    let literal_values = parts
        .iter()
        .filter_map(|part| match part {
            Expression::Literal {
                value: LiteralValue::String(value),
            } => Some(value.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(literal_values.iter().any(|value| value.contains("Hello")));
}

#[test]
fn binop_add_produces_concat_expression_with_flattened_args() {
    let plan = parse(&wrap_in_main("msg = a + \" \" + b"));
    let Step::AssignStep { target, value } = &plan.steps[0] else {
        panic!("expected assign step");
    };

    assert_eq!(target, &"msg".to_string());
    assert_eq!(
        value,
        &Expression::ConcatExpression {
            parts: vec![access_path("a"), literal(json!(" ")), access_path("b")]
        }
    );
}

#[test]
fn numeric_add_produces_binary_expression() {
    let plan = parse(&wrap_in_main("total = a + b"));
    let Step::AssignStep { target, value } = &plan.steps[0] else {
        panic!("expected assign step");
    };

    assert_eq!(target, &"total".to_string());
    assert_eq!(
        value,
        &Expression::BinaryExpression {
            operator: BinaryOperator::Add,
            left: Box::new(access_path("a")),
            right: Box::new(access_path("b")),
        }
    );
}

#[test]
fn if_else_produces_switch_step_with_two_branches() {
    let plan = parse(&wrap_in_main(
        "if x:\n    do_something(x)\nelse:\n    do_other(x)",
    ));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };

    assert_eq!(branch_labels(branches), vec!["if", "else"]);
    assert_eq!(branch_by_kind(branches, BranchKind::If).steps.len(), 1);
    assert_eq!(branch_by_kind(branches, BranchKind::Else).steps.len(), 1);
    assert!(matches!(
        branch_by_kind(branches, BranchKind::If).steps[0],
        Step::ToolStep { .. }
    ));
    assert!(matches!(
        branch_by_kind(branches, BranchKind::Else).steps[0],
        Step::ToolStep { .. }
    ));
}

#[test]
fn switch_step_if_condition_is_access_path() {
    let plan = parse(&wrap_in_main(
        "if x:\n    do_something(x)\nelse:\n    do_other(x)",
    ));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };

    assert_eq!(
        branch_by_kind(branches, BranchKind::If).condition,
        Some(access_path("x"))
    );
}

#[test]
fn else_branch_has_no_condition() {
    let plan = parse(&wrap_in_main(
        "if x:\n    do_something(x)\nelse:\n    do_other(x)",
    ));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };

    assert!(
        branch_by_kind(branches, BranchKind::Else)
            .condition
            .is_none()
    );
}

#[test]
fn if_elif_else_produces_single_switch_with_three_branches() {
    let plan = parse(&wrap_in_main(
        "if a:\n    tool_a(a)\nelif b:\n    tool_b(b)\nelse:\n    tool_c()",
    ));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };

    assert_eq!(branch_labels(branches), vec!["if", "elif", "else"]);
    assert_eq!(branches.len(), 3);
}

#[test]
fn multiple_elifs_are_numbered() {
    let plan = parse(&wrap_in_main(
        "if a:\n    tool_a()\nelif b:\n    tool_b()\nelif c:\n    tool_c()\nelse:\n    tool_d()",
    ));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };

    assert_eq!(branch_labels(branches), vec!["if", "elif", "elif", "else"]);
}

#[test]
fn for_loop_produces_loop_step() {
    let plan = parse(&wrap_in_main("for item in items:\n    process(item)"));
    let Step::LoopStep {
        iterable_variable,
        item_variable,
        body,
    } = &plan.steps[0]
    else {
        panic!("expected loop step");
    };

    assert_eq!(iterable_variable, "items");
    assert_eq!(item_variable, "item");
    assert_eq!(body.len(), 1);
    assert!(matches!(&body[0], Step::ToolStep { tool_id, .. } if tool_id == "process"));
}

#[test]
fn if_inside_for_loop() {
    let plan = parse(&wrap_in_main(
        "for item in items:\n    if item:\n        handle_true(item)\n    else:\n        handle_false(item)",
    ));

    let Step::LoopStep { body, .. } = &plan.steps[0] else {
        panic!("expected loop step");
    };
    assert_eq!(body.len(), 1);
    let Step::SwitchStep { branches } = &body[0] else {
        panic!("expected switch step");
    };
    assert_eq!(branch_labels(branches), vec!["if", "else"]);
}

#[test]
fn for_loop_inside_if() {
    let plan = parse(&wrap_in_main(
        "if has_items:\n    for item in items:\n        process(item)\nelse:\n    do_nothing()",
    ));

    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };
    let Step::LoopStep {
        iterable_variable, ..
    } = &branch_by_kind(branches, BranchKind::If).steps[0]
    else {
        panic!("expected loop step");
    };
    assert_eq!(iterable_variable, "items");
}

#[test]
fn imports_and_returns_do_not_crash() {
    let code = "import os\nfrom sys import path\n\ndef main():\n    x = \"hello\"\n    return x\n";
    let plan = parse(code);

    assert_eq!(plan.steps.len(), 1);
    assert_eq!(
        plan.steps[0],
        Step::AssignStep {
            target: "x".to_string(),
            value: literal(json!("hello")),
        }
    );
}

#[test]
fn raises_error_when_main_is_missing() {
    let error = parse_python_to_plan("def not_main():\n    x = \"hello\"\n", "main")
        .expect_err("should fail");
    assert!(matches!(error, ReadyError::PlanParsing(message) if message.contains("main")));
}

#[test]
fn keyword_args_are_included_after_positional() {
    let plan = parse(&wrap_in_main("result = tool(x, key=\"val\")"));
    let Step::ToolStep { arguments, .. } = &plan.steps[0] else {
        panic!("expected tool step");
    };

    assert_eq!(arguments, &vec![access_path("x"), literal(json!("val"))]);
}

#[test]
fn five_sequential_operations() {
    let plan = parse(&wrap_in_main(
        "a = \"hello\"\nb = fetch_data(a)\nc = process(b)\nd = format_result(c)\nsend_output(d)",
    ));

    assert_eq!(plan.steps.len(), 5);
    assert!(matches!(plan.steps[0], Step::AssignStep { .. }));
    assert!(matches!(plan.steps[1], Step::ToolStep { .. }));
    assert!(matches!(plan.steps[2], Step::ToolStep { .. }));
    assert!(matches!(plan.steps[3], Step::ToolStep { .. }));
    assert!(matches!(
        plan.steps[4],
        Step::ToolStep {
            output_variable: None,
            ..
        }
    ));
}

#[test]
fn pass_only_produces_empty_steps() {
    let plan = parse(&wrap_in_main("pass"));
    assert!(plan.steps.is_empty());
}

#[test]
fn annotated_string_assignment_produces_assign_step() {
    let plan = parse(&wrap_in_main("x: str = \"hello\""));
    assert_eq!(
        plan.steps[0],
        Step::AssignStep {
            target: "x".to_string(),
            value: literal(json!("hello")),
        }
    );
}

#[test]
fn constant_types_in_assignments() {
    for (literal_code, expected_value) in [
        ("42", json!(42)),
        ("3.14", json!(3.14)),
        ("True", json!(true)),
        ("False", json!(false)),
        ("None", json!(null)),
    ] {
        let plan = parse(&wrap_in_main(&format!("x = {literal_code}")));
        assert_eq!(
            plan.steps[0],
            Step::AssignStep {
                target: "x".to_string(),
                value: literal(expected_value),
            }
        );
    }
}

#[test]
fn simple_comparison_produces_comparison_expression() {
    let plan = parse(&wrap_in_main("if x > 5:\n    do_a()\nelse:\n    do_b()"));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };
    assert_eq!(
        branch_by_kind(branches, BranchKind::If).condition,
        Some(Expression::Comparison {
            operator: ComparisonOperator::GreaterThan,
            left: Box::new(access_path("x")),
            right: Box::new(literal(json!(5))),
        })
    );
}

#[test]
fn bare_variable_condition_produces_access_path() {
    let plan = parse(&wrap_in_main("if active:\n    run()"));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };
    assert_eq!(
        branch_by_kind(branches, BranchKind::If).condition,
        Some(access_path("active"))
    );
}

#[test]
fn if_elif_else_produces_structured_conditions() {
    let plan = parse(&wrap_in_main(
        "if a > 10:\n    tool_a()\nelif a > 5:\n    tool_b()\nelse:\n    tool_c()",
    ));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };

    assert_eq!(
        branch_by_kind(branches, BranchKind::If).condition,
        Some(Expression::Comparison {
            operator: ComparisonOperator::GreaterThan,
            left: Box::new(access_path("a")),
            right: Box::new(literal(json!(10))),
        })
    );
    assert_eq!(
        branch_by_kind(branches, BranchKind::ElseIf).condition,
        Some(Expression::Comparison {
            operator: ComparisonOperator::GreaterThan,
            left: Box::new(access_path("a")),
            right: Box::new(literal(json!(5))),
        })
    );
    assert!(
        branch_by_kind(branches, BranchKind::Else)
            .condition
            .is_none()
    );
}

#[test]
fn multiple_elifs_have_structured_conditions() {
    let plan = parse(&wrap_in_main(
        "if x == 1:\n    handle_one()\nelif x == 2:\n    handle_two()\nelif x == 3:\n    handle_three()\nelse:\n    handle_default()",
    ));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };

    assert_eq!(
        branch_by_kind(branches, BranchKind::If).condition,
        Some(Expression::Comparison {
            operator: ComparisonOperator::Equal,
            left: Box::new(access_path("x")),
            right: Box::new(literal(json!(1))),
        })
    );
    assert_eq!(
        branch_by_kind(branches, BranchKind::ElseIf).condition,
        Some(Expression::Comparison {
            operator: ComparisonOperator::Equal,
            left: Box::new(access_path("x")),
            right: Box::new(literal(json!(2))),
        })
    );
    assert_eq!(branches[2].kind, BranchKind::ElseIf);
    assert_eq!(
        branches[2].condition,
        Some(Expression::Comparison {
            operator: ComparisonOperator::Equal,
            left: Box::new(access_path("x")),
            right: Box::new(literal(json!(3))),
        })
    );
}

#[test]
fn elif_without_else_has_structured_conditions() {
    let plan = parse(&wrap_in_main("if a:\n    tool_a()\nelif b:\n    tool_b()"));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };

    assert_eq!(
        branch_by_kind(branches, BranchKind::If).condition,
        Some(access_path("a"))
    );
    assert_eq!(
        branch_by_kind(branches, BranchKind::ElseIf).condition,
        Some(access_path("b"))
    );
    assert!(!branch_labels(branches).contains(&"else"));
}

#[test]
fn equality_comparison() {
    let plan = parse(&wrap_in_main("if x == 42:\n    do_it()"));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };
    assert!(matches!(
        branch_by_kind(branches, BranchKind::If).condition,
        Some(Expression::Comparison { ref operator, .. }) if *operator == ComparisonOperator::Equal
    ));
}

#[test]
fn not_equal_comparison() {
    let plan = parse(&wrap_in_main("if x != 0:\n    do_it()"));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };
    assert!(matches!(
        branch_by_kind(branches, BranchKind::If).condition,
        Some(Expression::Comparison { ref operator, .. }) if *operator == ComparisonOperator::NotEqual
    ));
}

#[test]
fn boolean_and_condition() {
    let plan = parse(&wrap_in_main("if a and b:\n    do_it()"));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };
    assert_eq!(
        branch_by_kind(branches, BranchKind::If).condition,
        Some(Expression::Boolean {
            operator: BooleanOperator::And,
            operands: vec![access_path("a"), access_path("b")],
        })
    );
}

#[test]
fn boolean_or_condition() {
    let plan = parse(&wrap_in_main("if a or b:\n    do_it()"));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };
    assert_eq!(
        branch_by_kind(branches, BranchKind::If).condition,
        Some(Expression::Boolean {
            operator: BooleanOperator::Or,
            operands: vec![access_path("a"), access_path("b")],
        })
    );
}

#[test]
fn not_condition() {
    let plan = parse(&wrap_in_main("if not done:\n    do_it()"));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };
    assert_eq!(
        branch_by_kind(branches, BranchKind::If).condition,
        Some(Expression::Not {
            operand: Box::new(access_path("done")),
        })
    );
}

#[test]
fn literal_true_condition() {
    let plan = parse(&wrap_in_main("if True:\n    do_it()"));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };
    assert_eq!(
        branch_by_kind(branches, BranchKind::If).condition,
        Some(literal(json!(true)))
    );
}

#[test]
fn chained_comparison_produces_boolean_and() {
    let plan = parse(&wrap_in_main("if 0 < x < 10:\n    do_it()"));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };
    let Some(Expression::Boolean { operator, operands }) =
        &branch_by_kind(branches, BranchKind::If).condition
    else {
        panic!("expected boolean condition");
    };
    assert_eq!(operator, &BooleanOperator::And);
    assert_eq!(operands.len(), 2);
    assert!(matches!(operands[0], Expression::Comparison { .. }));
    assert!(matches!(operands[1], Expression::Comparison { .. }));
}

#[test]
fn function_call_in_condition_raises_value_error() {
    let error = parse_python_to_plan(&wrap_in_main("if some_func():\n    do_it()"), "main")
        .expect_err("should fail");
    assert!(
        matches!(error, ReadyError::PlanParsing(message) if message.contains("function calls"))
    );
}

#[test]
fn while_with_comparison() {
    let plan = parse(&wrap_in_main(
        "x = get_value()\nwhile x > 0:\n    process(x)\n    x = get_value()",
    ));

    assert_eq!(plan.steps.len(), 2);
    let Step::WhileStep { condition, body } = &plan.steps[1] else {
        panic!("expected while step");
    };
    assert!(
        matches!(condition, Expression::Comparison { operator, .. } if *operator == ComparisonOperator::GreaterThan)
    );
    assert_eq!(body.len(), 2);
}

#[test]
fn while_with_not() {
    let plan = parse(&wrap_in_main("while not is_done:\n    do_step()"));
    let Step::WhileStep { condition, .. } = &plan.steps[0] else {
        panic!("expected while step");
    };
    assert_eq!(
        condition,
        &Expression::Not {
            operand: Box::new(access_path("is_done")),
        }
    );
}

#[test]
fn while_with_bare_variable() {
    let plan = parse(&wrap_in_main("while running:\n    do_work()"));
    let Step::WhileStep { condition, .. } = &plan.steps[0] else {
        panic!("expected while step");
    };
    assert_eq!(condition, &access_path("running"));
}

#[test]
fn while_body_has_correct_steps() {
    let plan = parse(&wrap_in_main(
        "while active:\n    a = step_one()\n    b = step_two(a)\n    step_three(b)",
    ));
    let Step::WhileStep { body, .. } = &plan.steps[0] else {
        panic!("expected while step");
    };
    assert_eq!(body.len(), 3);
    assert!(matches!(&body[0], Step::ToolStep { tool_id, .. } if tool_id == "step_one"));
    assert!(matches!(&body[1], Step::ToolStep { tool_id, .. } if tool_id == "step_two"));
    assert!(matches!(&body[2], Step::ToolStep { tool_id, .. } if tool_id == "step_three"));
}

#[test]
fn while_with_nested_if() {
    let plan = parse(&wrap_in_main(
        "while running:\n    if condition:\n        handle_true()\n    else:\n        handle_false()",
    ));
    let Step::WhileStep { body, .. } = &plan.steps[0] else {
        panic!("expected while step");
    };
    assert_eq!(body.len(), 1);
    assert!(matches!(body[0], Step::SwitchStep { .. }));
}

#[test]
fn mod_emits_assign_with_binary_expression() {
    let plan = parse(&wrap_in_main("x = a % 3"));
    assert!(matches!(
        plan.steps[0],
        Step::AssignStep {
            value: Expression::BinaryExpression { ref operator, .. },
            ..
        } if *operator == BinaryOperator::Modulo
    ));
}

#[test]
fn mod_left_is_access_path() {
    let plan = parse(&wrap_in_main("x = a % 3"));
    let Step::AssignStep { value, .. } = &plan.steps[0] else {
        panic!("expected assign step");
    };
    let Expression::BinaryExpression { left, .. } = value else {
        panic!("expected binary expression");
    };
    assert_eq!(left.as_ref(), &access_path("a"));
}

#[test]
fn mod_right_is_literal() {
    let plan = parse(&wrap_in_main("x = a % 3"));
    let Step::AssignStep { value, .. } = &plan.steps[0] else {
        panic!("expected assign step");
    };
    let Expression::BinaryExpression { right, .. } = value else {
        panic!("expected binary expression");
    };
    assert_eq!(right.as_ref(), &literal(json!(3)));
}

#[test]
fn sub_emits_assign_with_binary_expression() {
    let plan = parse(&wrap_in_main("x = a - b"));
    assert!(matches!(
        plan.steps[0],
        Step::AssignStep {
            value: Expression::BinaryExpression { ref operator, .. },
            ..
        } if *operator == BinaryOperator::Subtract
    ));
}

#[test]
fn sub_has_two_access_path_args() {
    let plan = parse(&wrap_in_main("x = a - b"));
    let Step::AssignStep { value, .. } = &plan.steps[0] else {
        panic!("expected assign step");
    };
    let Expression::BinaryExpression { left, right, .. } = value else {
        panic!("expected binary expression");
    };
    assert_eq!(left.as_ref(), &access_path("a"));
    assert_eq!(right.as_ref(), &access_path("b"));
}

#[test]
fn mul_emits_assign_with_binary_expression() {
    let plan = parse(&wrap_in_main("x = a * 2"));
    assert!(matches!(
        plan.steps[0],
        Step::AssignStep {
            value: Expression::BinaryExpression { ref operator, .. },
            ..
        } if *operator == BinaryOperator::Multiply
    ));
}

#[test]
fn floordiv_emits_assign_with_binary_expression() {
    let plan = parse(&wrap_in_main("x = a // 4"));
    assert!(matches!(
        plan.steps[0],
        Step::AssignStep {
            value: Expression::BinaryExpression { ref operator, .. },
            ..
        } if *operator == BinaryOperator::FloorDivide
    ));
}

#[test]
fn div_emits_assign_with_binary_expression() {
    let plan = parse(&wrap_in_main("x = a / 4"));
    assert_eq!(
        plan.steps[0],
        Step::AssignStep {
            target: "x".to_string(),
            value: Expression::BinaryExpression {
                operator: BinaryOperator::Divide,
                left: Box::new(access_path("a")),
                right: Box::new(literal(json!(4))),
            },
        }
    );
}

#[test]
fn pow_emits_assign_with_binary_expression() {
    let plan = parse(&wrap_in_main("x = a ** 4"));
    assert_eq!(
        plan.steps[0],
        Step::AssignStep {
            target: "x".to_string(),
            value: Expression::BinaryExpression {
                operator: BinaryOperator::Power,
                left: Box::new(access_path("a")),
                right: Box::new(literal(json!(4))),
            },
        }
    );
}

#[test]
fn unary_negation_emits_assign_with_unary_expression() {
    let plan = parse(&wrap_in_main("x = -a"));
    assert_eq!(
        plan.steps[0],
        Step::AssignStep {
            target: "x".to_string(),
            value: Expression::UnaryExpression {
                operator: UnaryOperator::Minus,
                operand: Box::new(access_path("a")),
            },
        }
    );
}

#[test]
fn arithmetic_no_output_variable_raises_value_error() {
    let error = parse_python_to_plan(&wrap_in_main("a % 2"), "main").expect_err("should fail");
    assert!(
        matches!(error, ReadyError::PlanParsing(message) if message.contains("Unsupported expression statement type: BinOp"))
    );
}

#[test]
fn mod_two_literals() {
    let plan = parse(&wrap_in_main("x = 10 % 3"));
    let Step::AssignStep { value, .. } = &plan.steps[0] else {
        panic!("expected assign step");
    };
    let Expression::BinaryExpression { left, right, .. } = value else {
        panic!("expected binary expression");
    };
    assert_eq!(left.as_ref(), &literal(json!(10)));
    assert_eq!(right.as_ref(), &literal(json!(3)));
}

#[test]
fn attribute_in_assignment() {
    let plan = parse(&wrap_in_main("x = obj.name"));
    let Step::AssignStep { value, .. } = &plan.steps[0] else {
        panic!("expected assign step");
    };
    let Expression::AccessPath {
        variable_name,
        accessors,
    } = value
    else {
        panic!("expected access path");
    };
    assert_eq!(variable_name, "obj");
    assert_eq!(accessors.len(), 1);
    assert_eq!(accessors[0], Accessor::Attribute("name".to_string()));
}

#[test]
fn subscript_int_in_assignment() {
    let plan = parse(&wrap_in_main("x = items[0]"));
    let Step::AssignStep { value, .. } = &plan.steps[0] else {
        panic!("expected assign step");
    };
    let Expression::AccessPath {
        variable_name,
        accessors,
    } = value
    else {
        panic!("expected access path");
    };
    assert_eq!(variable_name, "items");
    assert_eq!(accessors[0], Accessor::Index(0));
}

#[test]
fn subscript_str_in_assignment() {
    let plan = parse(&wrap_in_main("x = data[\"key\"]"));
    let Step::AssignStep { value, .. } = &plan.steps[0] else {
        panic!("expected assign step");
    };
    let Expression::AccessPath {
        variable_name,
        accessors,
    } = value
    else {
        panic!("expected access path");
    };
    assert_eq!(variable_name, "data");
    assert_eq!(accessors[0], Accessor::Key("key".to_string()));
}

#[test]
fn chained_access_in_assignment() {
    let plan = parse(&wrap_in_main("x = result.items[0]"));
    let Step::AssignStep { value, .. } = &plan.steps[0] else {
        panic!("expected assign step");
    };
    let Expression::AccessPath {
        variable_name,
        accessors,
    } = value
    else {
        panic!("expected access path");
    };
    assert_eq!(variable_name, "result");
    assert_eq!(accessors.len(), 2);
    assert_eq!(accessors[0], Accessor::Attribute("items".to_string()));
    assert_eq!(accessors[1], Accessor::Index(0));
}

#[test]
fn attribute_as_tool_argument() {
    let plan = parse(&wrap_in_main("send(user.email)"));
    let Step::ToolStep { arguments, .. } = &plan.steps[0] else {
        panic!("expected tool step");
    };
    let Expression::AccessPath {
        variable_name,
        accessors,
    } = &arguments[0]
    else {
        panic!("expected access path");
    };
    assert_eq!(variable_name, "user");
    assert_eq!(accessors[0], Accessor::Attribute("email".to_string()));
}

#[test]
fn subscript_as_tool_argument() {
    let plan = parse(&wrap_in_main("send(items[1])"));
    let Step::ToolStep { arguments, .. } = &plan.steps[0] else {
        panic!("expected tool step");
    };
    let Expression::AccessPath {
        variable_name,
        accessors,
    } = &arguments[0]
    else {
        panic!("expected access path");
    };
    assert_eq!(variable_name, "items");
    assert_eq!(accessors[0], Accessor::Index(1));
}

#[test]
fn attribute_in_condition() {
    let plan = parse(&wrap_in_main("if user.active:\n    do_it()"));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };
    let Some(Expression::AccessPath {
        variable_name,
        accessors,
    }) = &branch_by_kind(branches, BranchKind::If).condition
    else {
        panic!("expected access path condition");
    };
    assert_eq!(variable_name, "user");
    assert_eq!(accessors[0], Accessor::Attribute("active".to_string()));
}

#[test]
fn subscript_in_condition() {
    let plan = parse(&wrap_in_main("if flags[0]:\n    do_it()"));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };
    let Some(Expression::AccessPath {
        variable_name,
        accessors,
    }) = &branch_by_kind(branches, BranchKind::If).condition
    else {
        panic!("expected access path condition");
    };
    assert_eq!(variable_name, "flags");
    assert_eq!(accessors[0], Accessor::Index(0));
}

#[test]
fn chained_access_in_comparison() {
    let plan = parse(&wrap_in_main("if result.count > 0:\n    do_it()"));
    let Step::SwitchStep { branches } = &plan.steps[0] else {
        panic!("expected switch step");
    };
    let Some(Expression::Comparison { left, right, .. }) =
        &branch_by_kind(branches, BranchKind::If).condition
    else {
        panic!("expected comparison");
    };
    let Expression::AccessPath {
        variable_name,
        accessors,
    } = left.as_ref()
    else {
        panic!("expected access path");
    };
    assert_eq!(variable_name, "result");
    assert_eq!(accessors[0], Accessor::Attribute("count".to_string()));
    assert_eq!(right.as_ref(), &literal(json!(0)));
}

#[test]
fn unsupported_slice_raises() {
    let error =
        parse_python_to_plan(&wrap_in_main("x = items[1:3]"), "main").expect_err("should fail");
    assert!(
        matches!(error, ReadyError::PlanParsing(message) if message.contains("Unsupported subscript slice type"))
    );
}

#[test]
fn dict_with_access_path_value_produces_dict_expression() {
    let plan = parse(&wrap_in_main(
        "result = some_tool(\"prompt\", {\"plain_text\": message.text, \"schema\": {\"type\": \"string\"}})",
    ));
    let Step::ToolStep { arguments, .. } = &plan.steps[0] else {
        panic!("expected tool step");
    };
    assert_eq!(arguments.len(), 2);
    let Expression::DictExpression { entries } = &arguments[1] else {
        panic!("expected DictExpression, got {:?}", arguments[1]);
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].0, "plain_text");
    assert_eq!(
        entries[0].1,
        Expression::AccessPath {
            variable_name: "message".to_string(),
            accessors: vec![Accessor::Attribute("text".to_string())],
        }
    );
    // The nested schema dict is all literals, so it stays as a Literal
    assert!(matches!(entries[1].1, Expression::Literal { .. }));
}

#[test]
fn dict_with_all_literal_values_stays_as_literal() {
    let plan = parse(&wrap_in_main("x = {\"a\": 1, \"b\": \"hello\"}"));
    let Step::AssignStep { value, .. } = &plan.steps[0] else {
        panic!("expected assign step");
    };
    assert!(
        matches!(value, Expression::Literal { value: crate::plan::LiteralValue::Object(_) }),
        "expected Literal::Object, got {value:?}"
    );
}

#[test]
fn list_with_access_path_element_produces_array_expression() {
    let plan = parse(&wrap_in_main("x = [item.name, \"literal\"]"));
    let Step::AssignStep { value, .. } = &plan.steps[0] else {
        panic!("expected assign step");
    };
    let Expression::ArrayExpression { elements } = value else {
        panic!("expected ArrayExpression, got {value:?}");
    };
    assert_eq!(elements.len(), 2);
    assert_eq!(
        elements[0],
        Expression::AccessPath {
            variable_name: "item".to_string(),
            accessors: vec![Accessor::Attribute("name".to_string())],
        }
    );
    assert_eq!(elements[1], literal(json!("literal")));
}

#[test]
fn list_with_all_literal_elements_stays_as_literal() {
    let plan = parse(&wrap_in_main("x = [1, 2, 3]"));
    let Step::AssignStep { value, .. } = &plan.steps[0] else {
        panic!("expected assign step");
    };
    assert!(
        matches!(value, Expression::Literal { value: crate::plan::LiteralValue::Array(_) }),
        "expected Literal::Array, got {value:?}"
    );
}
