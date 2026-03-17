//! Expression evaluation against a runtime variable scope.

use std::collections::HashMap;

use serde_json::{Number, Value};

use crate::error::{ReadyError, Result};
use crate::plan::{
    Accessor, BinaryOperator, BooleanOperator, ComparisonOperator, Expression, UnaryOperator,
};

/// Evaluates an [`Expression`](src/plan.rs) against a variable scope and returns a JSON value.
/// Supports literals, access paths, concatenation, arithmetic, comparisons, boolean operations, and logical negation.
pub fn evaluate_expression(expr: &Expression, variables: &HashMap<String, Value>) -> Result<Value> {
    match expr {
        Expression::Literal { value } => Ok(value.to_json_value()),
        Expression::AccessPath {
            variable_name,
            accessors,
        } => resolve_access(variable_name, accessors, variables),
        Expression::BinaryExpression {
            operator,
            left,
            right,
        } => {
            let left = evaluate_expression(left, variables)?;
            let right = evaluate_expression(right, variables)?;
            evaluate_binary_expression(operator, &left, &right)
        }
        Expression::UnaryExpression { operator, operand } => {
            let operand = evaluate_expression(operand, variables)?;
            evaluate_unary_expression(operator, &operand)
        }
        Expression::ConcatExpression { parts } => {
            let mut result = String::new();
            for part in parts {
                let value = evaluate_expression(part, variables)?;
                result.push_str(&value_to_string(&value));
            }
            Ok(Value::String(result))
        }
        Expression::Comparison {
            operator,
            left,
            right,
        } => {
            let left = evaluate_expression(left, variables)?;
            let right = evaluate_expression(right, variables)?;
            Ok(Value::Bool(evaluate_comparison(operator, &left, &right)?))
        }
        Expression::Boolean { operator, operands } => {
            let value = match operator {
                BooleanOperator::And => {
                    let mut result = true;
                    for operand in operands {
                        if !result {
                            break;
                        }
                        let value = evaluate_expression(operand, variables)?;
                        result = is_truthy(&value);
                    }
                    result
                }
                BooleanOperator::Or => {
                    let mut result = false;
                    for operand in operands {
                        if result {
                            break;
                        }
                        let value = evaluate_expression(operand, variables)?;
                        result = is_truthy(&value);
                    }
                    result
                }
            };
            Ok(Value::Bool(value))
        }
        Expression::Not { operand } => {
            let value = evaluate_expression(operand, variables)?;
            Ok(Value::Bool(!is_truthy(&value)))
        }
    }
}

/// Converts a JSON value into a display-friendly string.
/// String values are returned without quotes, while all other values use their standard display form.
pub fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => value.to_string(),
    }
}

/// Returns whether a JSON value is truthy using Python-like semantics.
/// Null, `false`, zero, empty strings, and empty collections are falsey; everything else is truthy.
pub fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_i64().map_or_else(
            || number.as_f64().is_some_and(|value| value != 0.0),
            |value| value != 0,
        ),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

fn resolve_access(
    variable_name: &str,
    accessors: &[Accessor],
    variables: &HashMap<String, Value>,
) -> Result<Value> {
    let mut value = variables
        .get(variable_name)
        .cloned()
        .ok_or_else(|| ReadyError::Evaluation(format!("Undefined variable: '{variable_name}'")))?;

    for accessor in accessors {
        value = apply_accessor(&value, accessor)?;
    }

    Ok(value)
}

fn apply_accessor(value: &Value, accessor: &Accessor) -> Result<Value> {
    match accessor {
        Accessor::Attribute(key) => match value {
            Value::Object(map) => map
                .get(key)
                .cloned()
                .ok_or_else(|| ReadyError::Evaluation(format!("Attribute '{key}' not found"))),
            other => Err(ReadyError::Evaluation(format!(
                "Cannot access attribute '{key}' on {}",
                json_type_name(other)
            ))),
        },
        Accessor::Index(index) => match value {
            Value::Array(items) => {
                if *index < 0 {
                    return Err(ReadyError::Evaluation(format!(
                        "Array index out of range: {index}"
                    )));
                }

                items.get(*index as usize).cloned().ok_or_else(|| {
                    ReadyError::Evaluation(format!("Array index out of range: {index}"))
                })
            }
            other => Err(ReadyError::Evaluation(format!(
                "Cannot index into {}",
                json_type_name(other)
            ))),
        },
        Accessor::Key(key) => match value {
            Value::Object(map) => map
                .get(key)
                .cloned()
                .ok_or_else(|| ReadyError::Evaluation(format!("Object key '{key}' not found"))),
            other => Err(ReadyError::Evaluation(format!(
                "Cannot index into {}",
                json_type_name(other)
            ))),
        },
    }
}

fn evaluate_binary_expression(
    operator: &BinaryOperator,
    left: &Value,
    right: &Value,
) -> Result<Value> {
    match operator {
        BinaryOperator::Add => numeric_binary_op(left, right, |a, b| a + b, |a, b| a + b),
        BinaryOperator::Subtract => numeric_binary_op(left, right, |a, b| a - b, |a, b| a - b),
        BinaryOperator::Multiply => numeric_binary_op(left, right, |a, b| a * b, |a, b| a * b),
        BinaryOperator::Divide => {
            let (left, right) = as_f64_pair(left, right)?;
            Ok(number_value_from_f64(left / right)?)
        }
        BinaryOperator::FloorDivide => {
            if let (Some(left), Some(right)) = (left.as_i64(), right.as_i64()) {
                Ok(Value::Number(Number::from(left / right)))
            } else {
                let (left, right) = as_f64_pair(left, right)?;
                Ok(number_value_from_f64((left / right).floor())?)
            }
        }
        BinaryOperator::Modulo => numeric_binary_op(left, right, |a, b| a % b, |a, b| a % b),
        BinaryOperator::Power => {
            if let (Some(left), Some(right)) = (left.as_i64(), right.as_i64())
                && right >= 0
            {
                return Ok(Value::Number(Number::from(left.pow(right as u32))));
            }

            let (left, right) = as_f64_pair(left, right)?;
            Ok(number_value_from_f64(left.powf(right))?)
        }
    }
}

fn evaluate_unary_expression(operator: &UnaryOperator, operand: &Value) -> Result<Value> {
    match operator {
        UnaryOperator::Plus => Ok(if operand.is_i64() {
            Value::Number(Number::from(operand.as_i64().unwrap_or_default()))
        } else {
            number_value_from_f64(as_f64(operand)?)?
        }),
        UnaryOperator::Minus => Ok(if let Some(value) = operand.as_i64() {
            Value::Number(Number::from(-value))
        } else {
            number_value_from_f64(-as_f64(operand)?)?
        }),
    }
}

fn evaluate_comparison(operator: &ComparisonOperator, left: &Value, right: &Value) -> Result<bool> {
    match operator {
        ComparisonOperator::Equal => Ok(left == right),
        ComparisonOperator::NotEqual => Ok(left != right),
        ComparisonOperator::LessThan => compare_ordering(left, right, |ordering| ordering.is_lt()),
        ComparisonOperator::GreaterThan => {
            compare_ordering(left, right, |ordering| ordering.is_gt())
        }
        ComparisonOperator::LessThanOrEqual => {
            compare_ordering(left, right, |ordering| ordering.is_le())
        }
        ComparisonOperator::GreaterThanOrEqual => {
            compare_ordering(left, right, |ordering| ordering.is_ge())
        }
        ComparisonOperator::In => contains_value(right, left),
        ComparisonOperator::NotIn => Ok(!contains_value(right, left)?),
        ComparisonOperator::Is => Ok(left == right),
        ComparisonOperator::IsNot => Ok(left != right),
    }
}

fn contains_value(container: &Value, needle: &Value) -> Result<bool> {
    match container {
        Value::Array(items) => Ok(items.iter().any(|item| item == needle)),
        Value::Object(map) => {
            let key = needle.as_str().ok_or_else(|| {
                ReadyError::Evaluation("Object membership requires a string key".to_string())
            })?;
            Ok(map.contains_key(key))
        }
        Value::String(text) => {
            let needle = needle.as_str().ok_or_else(|| {
                ReadyError::Evaluation("String membership requires a string needle".to_string())
            })?;
            Ok(text.contains(needle))
        }
        other => Err(ReadyError::Evaluation(format!(
            "Operator 'in' not supported for {}",
            json_type_name(other)
        ))),
    }
}

fn compare_ordering(
    left: &Value,
    right: &Value,
    predicate: impl Fn(std::cmp::Ordering) -> bool,
) -> Result<bool> {
    if let (Some(left), Some(right)) = (left.as_str(), right.as_str()) {
        return Ok(predicate(left.cmp(right)));
    }

    let (left, right) = as_f64_pair(left, right)?;
    let ordering = left.partial_cmp(&right).ok_or_else(|| {
        ReadyError::Evaluation("Cannot compare non-finite numeric values".to_string())
    })?;
    Ok(predicate(ordering))
}

fn numeric_binary_op(
    left: &Value,
    right: &Value,
    int_op: impl Fn(i64, i64) -> i64,
    float_op: impl Fn(f64, f64) -> f64,
) -> Result<Value> {
    if let (Some(left), Some(right)) = (left.as_i64(), right.as_i64()) {
        Ok(Value::Number(Number::from(int_op(left, right))))
    } else {
        let (left, right) = as_f64_pair(left, right)?;
        Ok(number_value_from_f64(float_op(left, right))?)
    }
}

fn as_f64_pair(left: &Value, right: &Value) -> Result<(f64, f64)> {
    Ok((as_f64(left)?, as_f64(right)?))
}

fn as_f64(value: &Value) -> Result<f64> {
    value.as_f64().ok_or_else(|| {
        ReadyError::Evaluation(format!(
            "Expected numeric value, got {}",
            json_type_name(value)
        ))
    })
}

fn number_value_from_f64(value: f64) -> Result<Value> {
    Number::from_f64(value)
        .map(Value::Number)
        .ok_or_else(|| ReadyError::Evaluation(format!("Invalid floating-point result: {value}")))
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}
