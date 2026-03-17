use std::collections::HashSet;

use crate::plan::Expression;

/// Collects all variable names referenced anywhere within an expression tree.
pub fn collect_expression_variables(expr: &Expression) -> HashSet<String> {
    let mut found = HashSet::new();
    collect_expression_variables_into(expr, &mut found);
    found
}

pub(crate) fn collect_expression_variables_into(expr: &Expression, found: &mut HashSet<String>) {
    match expr {
        Expression::AccessPath { variable_name, .. } => {
            found.insert(variable_name.clone());
        }
        Expression::BinaryExpression { left, right, .. }
        | Expression::Comparison { left, right, .. } => {
            collect_expression_variables_into(left, found);
            collect_expression_variables_into(right, found);
        }
        Expression::UnaryExpression { operand, .. } | Expression::Not { operand } => {
            collect_expression_variables_into(operand, found);
        }
        Expression::ConcatExpression { parts }
        | Expression::Boolean {
            operands: parts, ..
        } => {
            for part in parts {
                collect_expression_variables_into(part, found);
            }
        }
        Expression::Literal { .. } => {}
    }
}
