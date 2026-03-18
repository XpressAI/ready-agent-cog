use crate::error::{ReadyError, Result};
use crate::plan::{
    Accessor, BinaryOperator, BooleanOperator, Expression, LiteralValue, UnaryOperator,
};
use rustpython_parser::ast;

use super::{
    constant_to_literal, expression_name, extract_json_object_key, extract_subscript_key,
};

pub(crate) fn convert_expression(expr: &ast::Expr) -> Result<Expression> {
    match expr {
        ast::Expr::Constant(constant) => Ok(Expression::Literal {
            value: constant_to_literal(&constant.value)?,
        }),
        ast::Expr::Name(name) => Ok(Expression::AccessPath {
            variable_name: name.id.to_string(),
            accessors: Vec::new(),
        }),
        ast::Expr::Attribute(_) | ast::Expr::Subscript(_) => convert_access_path(expr),
        ast::Expr::FormattedValue(value) => convert_expression(&value.value),
        ast::Expr::JoinedStr(joined) => Ok(Expression::ConcatExpression {
            parts: joined
                .values
                .iter()
                .map(convert_expression)
                .collect::<Result<Vec<_>>>()?,
        }),
        ast::Expr::BinOp(bin_op) => convert_bin_op(bin_op),
        ast::Expr::UnaryOp(unary) => convert_unary_expression(unary),
        ast::Expr::Compare(compare) => super::conditions::convert_compare(compare),
        ast::Expr::BoolOp(bool_op) => Ok(Expression::Boolean {
            operator: match bool_op.op {
                ast::BoolOp::And => BooleanOperator::And,
                ast::BoolOp::Or => BooleanOperator::Or,
            },
            operands: bool_op
                .values
                .iter()
                .map(convert_expression)
                .collect::<Result<Vec<_>>>()?,
        }),
        ast::Expr::List(list) => convert_array(&list.elts),
        ast::Expr::Tuple(tuple) => convert_array(&tuple.elts),
        ast::Expr::Dict(dict) => convert_dict(dict),
        other => Err(ReadyError::PlanParsing(format!(
            "'{}' is not supported — only literals, variable names, attribute/index access, arithmetic, boolean expressions, f-strings, lists, and dicts are allowed",
            expression_name(other)
        ))),
    }
}

fn convert_bin_op(bin_op: &ast::ExprBinOp) -> Result<Expression> {
    if matches!(bin_op.op, ast::Operator::Add)
        && is_string_concatenation(&ast::Expr::BinOp(bin_op.clone()))
    {
        return Ok(Expression::ConcatExpression {
            parts: collapse_add_chain(&ast::Expr::BinOp(bin_op.clone()))
                .into_iter()
                .map(convert_expression)
                .collect::<Result<Vec<_>>>()?,
        });
    }

    Ok(Expression::BinaryExpression {
        operator: binary_operator(&bin_op.op)?,
        left: Box::new(convert_expression(&bin_op.left)?),
        right: Box::new(convert_expression(&bin_op.right)?),
    })
}

fn convert_unary_expression(unary: &ast::ExprUnaryOp) -> Result<Expression> {
    match unary.op {
        ast::UnaryOp::Not => Ok(Expression::Not {
            operand: Box::new(convert_expression(&unary.operand)?),
        }),
        ast::UnaryOp::USub => Ok(Expression::UnaryExpression {
            operator: UnaryOperator::Minus,
            operand: Box::new(convert_expression(&unary.operand)?),
        }),
        ast::UnaryOp::UAdd => Ok(Expression::UnaryExpression {
            operator: UnaryOperator::Plus,
            operand: Box::new(convert_expression(&unary.operand)?),
        }),
        _ => Err(ReadyError::PlanParsing(format!(
            "unary operator '{:?}' is not supported — only unary + and - are allowed",
            unary.op
        ))),
    }
}

fn convert_access_path(expr: &ast::Expr) -> Result<Expression> {
    let (variable_name, accessors) = unwind_access_chain(expr)?;
    Ok(Expression::AccessPath {
        variable_name,
        accessors,
    })
}

fn unwind_access_chain(expr: &ast::Expr) -> Result<(String, Vec<Accessor>)> {
    let mut accessors = Vec::new();
    let mut current = expr;

    loop {
        match current {
            ast::Expr::Attribute(attribute) => {
                accessors.push(Accessor::Attribute(attribute.attr.to_string()));
                current = &attribute.value;
            }
            ast::Expr::Subscript(subscript) => {
                accessors.push(extract_subscript_key(&subscript.slice)?);
                current = &subscript.value;
            }
            ast::Expr::Name(name) => {
                accessors.reverse();
                return Ok((name.id.to_string(), accessors));
            }
            other => {
                return Err(ReadyError::PlanParsing(format!(
                    "access chain must start from a variable name, not a '{}' — only `var.attr` or `var[key]` chains are allowed",
                    expression_name(other)
                )));
            }
        }
    }
}

fn convert_array(elements: &[ast::Expr]) -> Result<Expression> {
    let converted: Vec<Expression> = elements
        .iter()
        .map(convert_expression)
        .collect::<Result<Vec<_>>>()?;

    // If all elements are literals, keep the compact Literal representation
    if let Some(literals) = all_literals(&converted) {
        return Ok(Expression::Literal {
            value: LiteralValue::Array(literals),
        });
    }

    Ok(Expression::ArrayExpression {
        elements: converted,
    })
}

fn convert_dict(dict: &ast::ExprDict) -> Result<Expression> {
    let mut entries = Vec::new();
    for (key, value) in dict.keys.iter().zip(dict.values.iter()) {
        let Some(key_expr) = key else {
            return Err(ReadyError::PlanParsing(
                "Dictionary unpacking is not supported".to_string(),
            ));
        };
        let key = extract_json_object_key(key_expr)?;
        entries.push((key, convert_expression(value)?));
    }

    // If all values are literals, keep the compact Literal representation
    let all_literal = entries.iter().all(|(_, v)| matches!(v, Expression::Literal { .. }));
    if all_literal {
        let object = entries
            .into_iter()
            .map(|(k, v)| match v {
                Expression::Literal { value } => (k, value),
                _ => unreachable!(),
            })
            .collect();
        return Ok(Expression::Literal {
            value: LiteralValue::Object(object),
        });
    }

    Ok(Expression::DictExpression { entries })
}

/// Returns `Some(literals)` if every expression is a `Literal`, otherwise `None`.
fn all_literals(exprs: &[Expression]) -> Option<Vec<LiteralValue>> {
    exprs
        .iter()
        .map(|e| match e {
            Expression::Literal { value } => Some(value.clone()),
            _ => None,
        })
        .collect()
}

fn collapse_add_chain(expr: &ast::Expr) -> Vec<&ast::Expr> {
    match expr {
        ast::Expr::BinOp(bin_op) if matches!(bin_op.op, ast::Operator::Add) => {
            let mut parts = collapse_add_chain(&bin_op.left);
            parts.extend(collapse_add_chain(&bin_op.right));
            parts
        }
        other => vec![other],
    }
}

fn is_string_concatenation(expr: &ast::Expr) -> bool {
    collapse_add_chain(expr).into_iter().any(|part| match part {
        ast::Expr::Constant(constant) => matches!(constant.value, ast::Constant::Str(_)),
        ast::Expr::JoinedStr(_) => true,
        _ => false,
    })
}

fn binary_operator(operator: &ast::Operator) -> Result<BinaryOperator> {
    match operator {
        ast::Operator::Add => Ok(BinaryOperator::Add),
        ast::Operator::Sub => Ok(BinaryOperator::Subtract),
        ast::Operator::Mult => Ok(BinaryOperator::Multiply),
        ast::Operator::Div => Ok(BinaryOperator::Divide),
        ast::Operator::Mod => Ok(BinaryOperator::Modulo),
        ast::Operator::Pow => Ok(BinaryOperator::Power),
        ast::Operator::FloorDiv => Ok(BinaryOperator::FloorDivide),
        other => Err(ReadyError::PlanParsing(format!(
            "binary operator '{:?}' is not supported — allowed operators are +, -, *, /, //, %, **",
            other
        ))),
    }
}
