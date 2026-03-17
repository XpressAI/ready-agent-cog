use std::collections::HashMap;

use serde_json::{Value, json};

use super::evaluator::evaluate_expression;
use crate::error::ReadyError;
use crate::plan::{
    Accessor, BinaryOperator, BooleanOperator, ComparisonOperator, Expression, LiteralValue,
    UnaryOperator,
};

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
    fn to_literal(value: Value) -> LiteralValue {
        match value {
            Value::Null => LiteralValue::Null,
            Value::Bool(value) => LiteralValue::Bool(value),
            Value::Number(number) => {
                if let Some(value) = number.as_i64() {
                    LiteralValue::Integer(value)
                } else {
                    LiteralValue::Float(number.as_f64().expect("finite json number"))
                }
            }
            Value::String(value) => LiteralValue::String(value),
            Value::Array(values) => {
                LiteralValue::Array(values.into_iter().map(to_literal).collect())
            }
            Value::Object(values) => LiteralValue::Object(
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

fn eval(expr: &Expression, variables: &[(&str, Value)]) -> Value {
    evaluate_expression(expr, &scope(variables)).expect("expression should evaluate")
}

fn eval_err(expr: &Expression, variables: &[(&str, Value)]) -> ReadyError {
    evaluate_expression(expr, &scope(variables)).expect_err("expression should fail")
}

#[test]
fn returns_value_from_scope_for_string_variable() {
    assert_eq!(
        eval(&access("greeting"), &[("greeting", json!("hello"))]),
        json!("hello")
    );
}

#[test]
fn returns_value_from_scope_for_integer_variable() {
    assert_eq!(eval(&access("count"), &[("count", json!(42))]), json!(42));
}

#[test]
fn returns_value_from_scope_for_null_variable() {
    assert_eq!(
        eval(&access("empty"), &[("empty", Value::Null)]),
        Value::Null
    );
}

#[test]
fn returns_value_from_scope_for_list_variable() {
    assert_eq!(
        eval(&access("items"), &[("items", json!([1, 2, 3]))]),
        json!([1, 2, 3])
    );
}

#[test]
fn returns_value_from_scope_for_dict_variable() {
    assert_eq!(
        eval(&access("data"), &[("data", json!({"key": "val"}))]),
        json!({"key": "val"})
    );
}

#[test]
fn undefined_variable_raises_for_missing_reference() {
    let error = eval_err(&access("missing"), &[]);
    assert!(
        matches!(error, ReadyError::Evaluation(message) if message.contains("Undefined variable: 'missing'"))
    );
}

#[test]
fn undefined_variable_raises_even_when_other_variables_exist() {
    let error = eval_err(&access("missing"), &[("other", json!("exists"))]);
    assert!(
        matches!(error, ReadyError::Evaluation(message) if message.contains("Undefined variable: 'missing'"))
    );
}

#[test]
fn literal_returns_string_value() {
    assert_eq!(eval(&literal(json!("world")), &[]), json!("world"));
}

#[test]
fn literal_returns_integer_value() {
    assert_eq!(eval(&literal(json!(99)), &[]), json!(99));
}

#[test]
fn literal_returns_float_value() {
    assert_eq!(eval(&literal(json!(3.14)), &[]), json!(3.14));
}

#[test]
fn literal_returns_boolean_value() {
    assert_eq!(eval(&literal(json!(true)), &[]), json!(true));
}

#[test]
fn literal_returns_null_value() {
    assert_eq!(eval(&literal(Value::Null), &[]), Value::Null);
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
fn comparison_equal_true() {
    assert_eq!(
        eval(&cmp(ComparisonOperator::Equal, json!(1), json!(1)), &[]),
        json!(true)
    );
}

#[test]
fn comparison_equal_false() {
    assert_eq!(
        eval(&cmp(ComparisonOperator::Equal, json!(1), json!(2)), &[]),
        json!(false)
    );
}

#[test]
fn comparison_not_equal_true() {
    assert_eq!(
        eval(&cmp(ComparisonOperator::NotEqual, json!(1), json!(2)), &[]),
        json!(true)
    );
}

#[test]
fn comparison_not_equal_false() {
    assert_eq!(
        eval(&cmp(ComparisonOperator::NotEqual, json!(1), json!(1)), &[]),
        json!(false)
    );
}

#[test]
fn comparison_less_than_true() {
    assert_eq!(
        eval(&cmp(ComparisonOperator::LessThan, json!(1), json!(2)), &[]),
        json!(true)
    );
}

#[test]
fn comparison_less_than_false() {
    assert_eq!(
        eval(&cmp(ComparisonOperator::LessThan, json!(2), json!(1)), &[]),
        json!(false)
    );
}

#[test]
fn comparison_greater_than_true() {
    assert_eq!(
        eval(
            &cmp(ComparisonOperator::GreaterThan, json!(2), json!(1)),
            &[]
        ),
        json!(true)
    );
}

#[test]
fn comparison_greater_than_false() {
    assert_eq!(
        eval(
            &cmp(ComparisonOperator::GreaterThan, json!(1), json!(2)),
            &[]
        ),
        json!(false)
    );
}

#[test]
fn comparison_less_equal_true() {
    assert_eq!(
        eval(
            &cmp(ComparisonOperator::LessThanOrEqual, json!(1), json!(1)),
            &[]
        ),
        json!(true)
    );
}

#[test]
fn comparison_less_equal_false() {
    assert_eq!(
        eval(
            &cmp(ComparisonOperator::LessThanOrEqual, json!(2), json!(1)),
            &[]
        ),
        json!(false)
    );
}

#[test]
fn comparison_greater_equal_true() {
    assert_eq!(
        eval(
            &cmp(ComparisonOperator::GreaterThanOrEqual, json!(1), json!(1)),
            &[]
        ),
        json!(true)
    );
}

#[test]
fn comparison_greater_equal_false() {
    assert_eq!(
        eval(
            &cmp(ComparisonOperator::GreaterThanOrEqual, json!(1), json!(2)),
            &[]
        ),
        json!(false)
    );
}

#[test]
fn membership_in_true() {
    assert_eq!(
        eval(
            &cmp(ComparisonOperator::In, json!("a"), json!(["a", "b"])),
            &[]
        ),
        json!(true)
    );
}

#[test]
fn membership_in_false() {
    assert_eq!(
        eval(
            &cmp(ComparisonOperator::In, json!("c"), json!(["a", "b"])),
            &[]
        ),
        json!(false)
    );
}

#[test]
fn membership_not_in_true() {
    assert_eq!(
        eval(
            &cmp(ComparisonOperator::NotIn, json!("c"), json!(["a", "b"])),
            &[]
        ),
        json!(true)
    );
}

#[test]
fn membership_not_in_false() {
    assert_eq!(
        eval(
            &cmp(ComparisonOperator::NotIn, json!("a"), json!(["a", "b"])),
            &[]
        ),
        json!(false)
    );
}

#[test]
fn identity_is_true_for_nulls() {
    assert_eq!(
        eval(&cmp(ComparisonOperator::Is, Value::Null, Value::Null), &[]),
        json!(true)
    );
}

#[test]
fn identity_is_not_false_for_nulls() {
    assert_eq!(
        eval(
            &cmp(ComparisonOperator::IsNot, Value::Null, Value::Null),
            &[]
        ),
        json!(false)
    );
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
fn boolean_and_true_true() {
    assert_eq!(
        eval(
            &boolean(BooleanOperator::And, vec![json!(true), json!(true)]),
            &[]
        ),
        json!(true)
    );
}

#[test]
fn boolean_and_true_false() {
    assert_eq!(
        eval(
            &boolean(BooleanOperator::And, vec![json!(true), json!(false)]),
            &[]
        ),
        json!(false)
    );
}

#[test]
fn boolean_and_false_true() {
    assert_eq!(
        eval(
            &boolean(BooleanOperator::And, vec![json!(false), json!(true)]),
            &[]
        ),
        json!(false)
    );
}

#[test]
fn boolean_or_true_false() {
    assert_eq!(
        eval(
            &boolean(BooleanOperator::Or, vec![json!(true), json!(false)]),
            &[]
        ),
        json!(true)
    );
}

#[test]
fn boolean_or_false_true() {
    assert_eq!(
        eval(
            &boolean(BooleanOperator::Or, vec![json!(false), json!(true)]),
            &[]
        ),
        json!(true)
    );
}

#[test]
fn boolean_or_false_false() {
    assert_eq!(
        eval(
            &boolean(BooleanOperator::Or, vec![json!(false), json!(false)]),
            &[]
        ),
        json!(false)
    );
}

#[test]
fn not_true_is_false() {
    assert_eq!(eval(&not(json!(true)), &[]), json!(false));
}

#[test]
fn not_false_is_true() {
    assert_eq!(eval(&not(json!(false)), &[]), json!(true));
}

#[test]
fn not_one_is_false() {
    assert_eq!(eval(&not(json!(1)), &[]), json!(false));
}

#[test]
fn not_zero_is_true() {
    assert_eq!(eval(&not(json!(0)), &[]), json!(true));
}

#[test]
fn not_non_empty_string_is_false() {
    assert_eq!(eval(&not(json!("non-empty")), &[]), json!(false));
}

#[test]
fn not_empty_string_is_true() {
    assert_eq!(eval(&not(json!("")), &[]), json!(true));
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
fn plain_access_path() {
    assert_eq!(eval(&access("val"), &[("val", json!(99))]), json!(99));
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
