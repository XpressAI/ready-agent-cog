use std::collections::HashMap;

use serde_json::{Value, json};

use super::evaluator::evaluate_expression;
use crate::error::ReadyError;
use crate::plan::{
    Accessor, BinaryOperator, BooleanOperator, ComparisonOperator, Expression, UnaryOperator,
};
use crate::test_helpers::to_literal;

fn scope(entries: &[(&str, Value)]) -> HashMap<String, Value> {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_string(), value.clone()))
        .collect()
}

fn access(variable_name: &str) -> Expression {
    Expression::AccessPath {
        variable_name: variable_name.to_string(),
        accessors: Vec::new(),
    }
}

fn literal(value: Value) -> Expression {
    Expression::Literal {
        value: to_literal(value),
    }
}

fn eval(expr: &Expression, variables: &[(&str, Value)]) -> Value {
    evaluate_expression(expr, &scope(variables)).expect("expression should evaluate")
}

fn eval_err(expr: &Expression, variables: &[(&str, Value)]) -> ReadyError {
    evaluate_expression(expr, &scope(variables)).expect_err("expression should fail")
}

#[test]
fn access_path_returns_value_from_scope_across_supported_json_types() {
    let cases = vec![
        ("greeting", json!("hello")),
        ("count", json!(42)),
        ("empty", Value::Null),
        ("items", json!([1, 2, 3])),
        ("data", json!({"key": "val"})),
    ];

    for (name, value) in cases {
        assert_eq!(eval(&access(name), &[(name, value.clone())]), value);
    }
}

#[test]
fn undefined_variable_raises_for_missing_reference() {
    let error = eval_err(&access("missing"), &[]);
    assert!(
        matches!(error, ReadyError::Evaluation(message) if message.contains("Undefined variable: 'missing'"))
    );
}

#[test]
fn literal_returns_value_across_supported_json_types() {
    let cases = vec![
        json!("world"),
        json!(99),
        json!(3.14),
        json!(true),
        Value::Null,
    ];

    for value in cases {
        assert_eq!(eval(&literal(value.clone()), &[]), value);
    }
}

#[test]
fn literal_does_not_consult_variables() {
    assert_eq!(
        eval(&literal(json!("fixed")), &[("fixed", json!("overridden"))]),
        json!("fixed")
    );
}

#[test]
fn concat_expression_combines_literal_and_variable() {
    let expr = Expression::ConcatExpression {
        parts: vec![literal(json!("Hello ")), access("name")],
    };
    assert_eq!(eval(&expr, &[("name", json!("Bob"))]), json!("Hello Bob"));
}

#[test]
fn binary_expression_addition() {
    let expr = Expression::BinaryExpression {
        operator: BinaryOperator::Add,
        left: Box::new(literal(json!(10))),
        right: Box::new(literal(json!(3))),
    };
    assert_eq!(eval(&expr, &[]), json!(13));
}

#[test]
fn binary_expression_division() {
    let expr = Expression::BinaryExpression {
        operator: BinaryOperator::Divide,
        left: Box::new(literal(json!(10))),
        right: Box::new(literal(json!(4))),
    };
    assert_eq!(eval(&expr, &[]), json!(2.5));
}

#[test]
fn binary_expression_exponentiation() {
    let expr = Expression::BinaryExpression {
        operator: BinaryOperator::Power,
        left: Box::new(literal(json!(2))),
        right: Box::new(literal(json!(3))),
    };
    assert_eq!(eval(&expr, &[]), json!(8));
}

#[test]
fn unary_expression_negation() {
    let expr = Expression::UnaryExpression {
        operator: UnaryOperator::Minus,
        operand: Box::new(literal(json!(5))),
    };
    assert_eq!(eval(&expr, &[]), json!(-5));
}

#[test]
fn binary_expression_modulo() {
    let expr = Expression::BinaryExpression {
        operator: BinaryOperator::Modulo,
        left: Box::new(literal(json!(10))),
        right: Box::new(literal(json!(3))),
    };
    assert_eq!(eval(&expr, &[]), json!(1));
}

#[test]
fn comparison_operators_follow_expected_truth_table() {
    let cases = vec![
        (ComparisonOperator::Equal, json!(1), json!(1), json!(true)),
        (ComparisonOperator::Equal, json!(1), json!(2), json!(false)),
        (
            ComparisonOperator::NotEqual,
            json!(1),
            json!(2),
            json!(true),
        ),
        (
            ComparisonOperator::NotEqual,
            json!(1),
            json!(1),
            json!(false),
        ),
        (
            ComparisonOperator::LessThan,
            json!(1),
            json!(2),
            json!(true),
        ),
        (
            ComparisonOperator::LessThan,
            json!(2),
            json!(1),
            json!(false),
        ),
        (
            ComparisonOperator::GreaterThan,
            json!(2),
            json!(1),
            json!(true),
        ),
        (
            ComparisonOperator::GreaterThan,
            json!(1),
            json!(2),
            json!(false),
        ),
        (
            ComparisonOperator::LessThanOrEqual,
            json!(1),
            json!(1),
            json!(true),
        ),
        (
            ComparisonOperator::LessThanOrEqual,
            json!(2),
            json!(1),
            json!(false),
        ),
        (
            ComparisonOperator::GreaterThanOrEqual,
            json!(1),
            json!(1),
            json!(true),
        ),
        (
            ComparisonOperator::GreaterThanOrEqual,
            json!(1),
            json!(2),
            json!(false),
        ),
        (
            ComparisonOperator::In,
            json!("a"),
            json!(["a", "b"]),
            json!(true),
        ),
        (
            ComparisonOperator::In,
            json!("c"),
            json!(["a", "b"]),
            json!(false),
        ),
        (
            ComparisonOperator::NotIn,
            json!("c"),
            json!(["a", "b"]),
            json!(true),
        ),
        (
            ComparisonOperator::NotIn,
            json!("a"),
            json!(["a", "b"]),
            json!(false),
        ),
        (
            ComparisonOperator::Is,
            Value::Null,
            Value::Null,
            json!(true),
        ),
        (
            ComparisonOperator::IsNot,
            Value::Null,
            Value::Null,
            json!(false),
        ),
    ];

    for (operator, left, right, expected) in cases {
        assert_eq!(eval(&cmp(operator, left, right), &[]), expected);
    }
}

#[test]
fn comparison_variable_vs_variable() {
    let expr = Expression::Comparison {
        operator: ComparisonOperator::Equal,
        left: Box::new(access("a")),
        right: Box::new(access("b")),
    };
    assert_eq!(
        eval(&expr, &[("a", json!(5)), ("b", json!(5))]),
        json!(true)
    );
    assert_eq!(
        eval(&expr, &[("a", json!(5)), ("b", json!(6))]),
        json!(false)
    );
}

#[test]
fn comparison_variable_vs_literal() {
    let expr = Expression::Comparison {
        operator: ComparisonOperator::GreaterThan,
        left: Box::new(access("score")),
        right: Box::new(literal(json!(50))),
    };
    assert_eq!(eval(&expr, &[("score", json!(75))]), json!(true));
    assert_eq!(eval(&expr, &[("score", json!(25))]), json!(false));
}

#[test]
fn boolean_operators_follow_expected_truth_table() {
    let cases = vec![
        (
            boolean(BooleanOperator::And, vec![json!(true), json!(true)]),
            json!(true),
        ),
        (
            boolean(BooleanOperator::And, vec![json!(true), json!(false)]),
            json!(false),
        ),
        (
            boolean(BooleanOperator::And, vec![json!(false), json!(true)]),
            json!(false),
        ),
        (
            boolean(BooleanOperator::Or, vec![json!(true), json!(false)]),
            json!(true),
        ),
        (
            boolean(BooleanOperator::Or, vec![json!(false), json!(true)]),
            json!(true),
        ),
        (
            boolean(BooleanOperator::Or, vec![json!(false), json!(false)]),
            json!(false),
        ),
    ];

    for (expr, expected) in cases {
        assert_eq!(eval(&expr, &[]), expected);
    }
}

#[test]
fn not_coerces_values_using_truthiness_rules() {
    let cases = vec![
        (json!(true), json!(false)),
        (json!(false), json!(true)),
        (json!(1), json!(false)),
        (json!(0), json!(true)),
        (json!("non-empty"), json!(false)),
        (json!(""), json!(true)),
    ];

    for (value, expected) in cases {
        assert_eq!(eval(&not(value), &[]), expected);
    }
}

#[test]
fn nested_comparison_inside_boolean_and() {
    let expr = Expression::Boolean {
        operator: BooleanOperator::And,
        operands: vec![
            Expression::Comparison {
                operator: ComparisonOperator::GreaterThan,
                left: Box::new(access("x")),
                right: Box::new(literal(json!(0))),
            },
            Expression::Comparison {
                operator: ComparisonOperator::LessThan,
                left: Box::new(access("y")),
                right: Box::new(literal(json!(10))),
            },
        ],
    };
    assert_eq!(
        eval(&expr, &[("x", json!(5)), ("y", json!(3))]),
        json!(true)
    );
    assert_eq!(
        eval(&expr, &[("x", json!(-1)), ("y", json!(3))]),
        json!(false)
    );
}

#[test]
fn nested_not_around_comparison() {
    let expr = Expression::Not {
        operand: Box::new(Expression::Comparison {
            operator: ComparisonOperator::Equal,
            left: Box::new(access("x")),
            right: Box::new(literal(json!(0))),
        }),
    };
    assert_eq!(eval(&expr, &[("x", json!(0))]), json!(false));
    assert_eq!(eval(&expr, &[("x", json!(1))]), json!(true));
}

#[test]
fn attribute_on_object() {
    let expr = Expression::AccessPath {
        variable_name: "pt".to_string(),
        accessors: vec![Accessor::Attribute("x".to_string())],
    };
    assert_eq!(eval(&expr, &[("pt", json!({"x": 3, "y": 7}))]), json!(3));
}

#[test]
fn index_on_list() {
    let expr = Expression::AccessPath {
        variable_name: "items".to_string(),
        accessors: vec![Accessor::Index(1)],
    };
    assert_eq!(eval(&expr, &[("items", json!([10, 20, 30]))]), json!(20));
}

#[test]
fn string_key_on_dict() {
    let expr = Expression::AccessPath {
        variable_name: "data".to_string(),
        accessors: vec![Accessor::Key("name".to_string())],
    };
    assert_eq!(
        eval(&expr, &[("data", json!({"name": "Alice"}))]),
        json!("Alice")
    );
}

#[test]
fn chained_access() {
    let expr = Expression::AccessPath {
        variable_name: "data".to_string(),
        accessors: vec![
            Accessor::Key("points".to_string()),
            Accessor::Index(0),
            Accessor::Attribute("y".to_string()),
        ],
    };
    assert_eq!(
        eval(
            &expr,
            &[(
                "data",
                json!({"points": [{"x": 1, "y": 2}, {"x": 3, "y": 4}]})
            )],
        ),
        json!(2)
    );
}

#[test]
fn undefined_base_raises() {
    let expr = Expression::AccessPath {
        variable_name: "missing".to_string(),
        accessors: vec![Accessor::Attribute("x".to_string())],
    };
    let error = eval_err(&expr, &[]);
    assert!(
        matches!(error, ReadyError::Evaluation(message) if message.contains("Undefined variable: 'missing'"))
    );
}

#[test]
fn bad_attribute_raises() {
    let expr = Expression::AccessPath {
        variable_name: "pt".to_string(),
        accessors: vec![Accessor::Attribute("z".to_string())],
    };
    let error = eval_err(&expr, &[("pt", json!({"x": 1, "y": 2}))]);
    assert!(
        matches!(error, ReadyError::Evaluation(message) if message.contains("Attribute 'z' not found"))
    );
}

#[test]
fn bad_index_raises() {
    let expr = Expression::AccessPath {
        variable_name: "items".to_string(),
        accessors: vec![Accessor::Index(99)],
    };
    let error = eval_err(&expr, &[("items", json!([1, 2, 3]))]);
    assert!(
        matches!(error, ReadyError::Evaluation(message) if message.contains("Array index out of range: 99"))
    );
}

#[test]
fn condition_with_attribute() {
    let expr = Expression::AccessPath {
        variable_name: "pt".to_string(),
        accessors: vec![Accessor::Attribute("x".to_string())],
    };
    assert_eq!(eval(&expr, &[("pt", json!({"x": 5, "y": 0}))]), json!(5));
}

#[test]
fn condition_with_subscript() {
    let expr = Expression::AccessPath {
        variable_name: "flags".to_string(),
        accessors: vec![Accessor::Key("active".to_string())],
    };
    assert_eq!(
        eval(&expr, &[("flags", json!({"active": true}))]),
        json!(true)
    );
}

#[test]
fn comparison_with_accessed_value() {
    let expr = Expression::Comparison {
        operator: ComparisonOperator::GreaterThan,
        left: Box::new(Expression::AccessPath {
            variable_name: "pt".to_string(),
            accessors: vec![Accessor::Attribute("x".to_string())],
        }),
        right: Box::new(literal(json!(5))),
    };
    assert_eq!(
        eval(&expr, &[("pt", json!({"x": 10, "y": 0}))]),
        json!(true)
    );
}

fn cmp(operator: ComparisonOperator, left: Value, right: Value) -> Expression {
    Expression::Comparison {
        operator,
        left: Box::new(literal(left)),
        right: Box::new(literal(right)),
    }
}

fn boolean(operator: BooleanOperator, values: Vec<Value>) -> Expression {
    Expression::Boolean {
        operator,
        operands: values.into_iter().map(literal).collect(),
    }
}

fn not(value: Value) -> Expression {
    Expression::Not {
        operand: Box::new(literal(value)),
    }
}
