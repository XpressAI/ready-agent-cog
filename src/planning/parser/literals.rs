use rustpython_parser::ast;
use serde_json::Number;

use crate::error::{ReadyError, Result};
use crate::plan::{Accessor, Expression, LiteralValue};

use super::{convert_expression, expression_name};

pub(crate) fn extract_string_literal(expr: &ast::Expr) -> Result<String> {
    match expr {
        ast::Expr::Constant(constant) => match &constant.value {
            ast::Constant::Str(value) => Ok(value.to_string()),
            other => Err(ReadyError::PlanParsing(format!(
                "Expected string literal, found {other:?}"
            ))),
        },
        other => Err(ReadyError::PlanParsing(format!(
            "Expected string literal, found {}",
            expression_name(other)
        ))),
    }
}

pub(crate) fn extract_json_object_key(expr: &ast::Expr) -> Result<String> {
    match expression_to_literal_value(expr)? {
        LiteralValue::String(value) => Ok(value),
        LiteralValue::Integer(value) => Ok(value.to_string()),
        LiteralValue::Float(value) => Ok(value.to_string()),
        LiteralValue::Bool(value) => Ok(value.to_string()),
        LiteralValue::Null => Ok("null".to_string()),
        other => Err(ReadyError::PlanParsing(format!(
            "Unsupported dictionary key literal: {other:?}"
        ))),
    }
}

pub(crate) fn expression_to_literal_value(expr: &ast::Expr) -> Result<LiteralValue> {
    match convert_expression(expr)? {
        Expression::Literal { value } => Ok(value),
        other => Err(ReadyError::PlanParsing(format!(
            "Expected literal expression, found {other:?}"
        ))),
    }
}

pub(crate) fn extract_subscript_key(expr: &ast::Expr) -> Result<Accessor> {
    match expr {
        ast::Expr::Constant(constant) => match constant_to_literal(&constant.value)? {
            LiteralValue::String(value) => Ok(Accessor::Key(value)),
            LiteralValue::Integer(value) => Ok(Accessor::Index(value)),
            LiteralValue::Float(_) => Err(ReadyError::PlanParsing(
                "Only integer subscript indices are supported in access paths".to_string(),
            )),
            other => Err(ReadyError::PlanParsing(format!(
                "Unsupported subscript key literal: {other:?}. Only string and integer keys are supported."
            ))),
        },
        other => Err(ReadyError::PlanParsing(format!(
            "Unsupported subscript slice type: {}. Only constant integer/string keys are supported.",
            expression_name(other)
        ))),
    }
}

pub(crate) fn constant_to_literal(constant: &ast::Constant) -> Result<LiteralValue> {
    match constant {
        ast::Constant::None => Ok(LiteralValue::Null),
        ast::Constant::Bool(value) => Ok(LiteralValue::Bool(*value)),
        ast::Constant::Str(value) => Ok(LiteralValue::String(value.to_string())),
        ast::Constant::Bytes(value) => Ok(LiteralValue::Array(
            value
                .iter()
                .map(|byte| LiteralValue::Integer(i64::from(*byte)))
                .collect(),
        )),
        ast::Constant::Int(value) => {
            let int_text = value.to_string();
            int_text
                .parse::<i64>()
                .map(LiteralValue::Integer)
                .map_err(|_| {
                    ReadyError::PlanParsing(format!(
                        "Integer literal out of supported typed AST range: {int_text}"
                    ))
                })
        }
        ast::Constant::Float(value) => Number::from_f64(*value)
            .map(|_| LiteralValue::Float(*value))
            .ok_or_else(|| ReadyError::PlanParsing(format!("Invalid float literal: {value}"))),
        ast::Constant::Complex { .. } | ast::Constant::Ellipsis | ast::Constant::Tuple(_) => Err(
            ReadyError::PlanParsing(format!("Unsupported literal constant: {constant:?}")),
        ),
    }
}

