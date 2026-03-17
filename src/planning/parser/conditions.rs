use rustpython_parser::ast;

use crate::error::{ReadyError, Result};
use crate::plan::{BooleanOperator, ComparisonOperator, Expression};

use super::{convert_expression, expression_name};

pub(crate) fn convert_condition(expr: &ast::Expr) -> Result<Expression> {
    match expr {
        ast::Expr::Compare(compare) => convert_compare(compare),
        ast::Expr::BoolOp(bool_op) => Ok(Expression::Boolean {
            operator: match bool_op.op {
                ast::BoolOp::And => BooleanOperator::And,
                ast::BoolOp::Or => BooleanOperator::Or,
            },
            operands: bool_op
                .values
                .iter()
                .map(convert_condition)
                .collect::<Result<Vec<_>>>()?,
        }),
        ast::Expr::UnaryOp(unary) if matches!(unary.op, ast::UnaryOp::Not) => Ok(Expression::Not {
            operand: Box::new(convert_condition(&unary.operand)?),
        }),
        ast::Expr::Call(call) => Err(ReadyError::PlanParsing(format!(
            "Conditions must not contain function calls. Found call to: {}",
            expression_name(&call.func)
        ))),
        _ => convert_expression(expr),
    }
}

pub(crate) fn convert_compare(compare: &ast::ExprCompare) -> Result<Expression> {
    if compare.ops.len() == 1 && compare.comparators.len() == 1 {
        return Ok(Expression::Comparison {
            operator: comparison_operator(&compare.ops[0]),
            left: Box::new(convert_condition(&compare.left)?),
            right: Box::new(convert_condition(&compare.comparators[0])?),
        });
    }

    let mut left = compare.left.as_ref();
    let mut operands = Vec::new();
    for (operator, comparator) in compare.ops.iter().zip(compare.comparators.iter()) {
        operands.push(Expression::Comparison {
            operator: comparison_operator(operator),
            left: Box::new(convert_condition(left)?),
            right: Box::new(convert_condition(comparator)?),
        });
        left = comparator;
    }

    Ok(Expression::Boolean {
        operator: BooleanOperator::And,
        operands,
    })
}

fn comparison_operator(operator: &ast::CmpOp) -> ComparisonOperator {
    match operator {
        ast::CmpOp::Eq => ComparisonOperator::Equal,
        ast::CmpOp::NotEq => ComparisonOperator::NotEqual,
        ast::CmpOp::Lt => ComparisonOperator::LessThan,
        ast::CmpOp::Gt => ComparisonOperator::GreaterThan,
        ast::CmpOp::LtE => ComparisonOperator::LessThanOrEqual,
        ast::CmpOp::GtE => ComparisonOperator::GreaterThanOrEqual,
        ast::CmpOp::In => ComparisonOperator::In,
        ast::CmpOp::NotIn => ComparisonOperator::NotIn,
        ast::CmpOp::Is => ComparisonOperator::Is,
        ast::CmpOp::IsNot => ComparisonOperator::IsNot,
    }
}
