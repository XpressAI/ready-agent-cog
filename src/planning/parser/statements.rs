use rustpython_parser::ast;

use crate::error::{ReadyError, Result};
use crate::plan::{BranchKind, ConditionalBranch, Step};

use super::{
    convert_condition, convert_expression, expression_name, extract_name_target,
    extract_string_literal, statement_name,
};

pub(crate) fn convert_body(body: &[ast::Stmt]) -> Result<Vec<Step>> {
    body.iter().filter_map(convert_statement).collect()
}

fn convert_statement(statement: &ast::Stmt) -> Option<Result<Step>> {
    Some(match statement {
        ast::Stmt::Assign(assign) => convert_assign(assign),
        ast::Stmt::AnnAssign(assign) => convert_ann_assign(assign),
        ast::Stmt::Expr(expr) => convert_expression_statement(expr),
        ast::Stmt::If(if_stmt) => convert_if(if_stmt),
        ast::Stmt::For(for_stmt) => convert_for(for_stmt),
        ast::Stmt::While(while_stmt) => convert_while(while_stmt),
        ast::Stmt::Return(_)
        | ast::Stmt::Import(_)
        | ast::Stmt::ImportFrom(_)
        | ast::Stmt::Pass(_) => {
            return None;
        }
        other => Err(ReadyError::PlanParsing(format!(
            "'{}' is not supported — only assignments, tool calls, if/elif/else, for/while loops, pass, and return are allowed",
            statement_name(other)
        ))),
    })
}

fn convert_assign(assign: &ast::StmtAssign) -> Result<Step> {
    if assign.targets.len() != 1 {
        return Err(ReadyError::PlanParsing(
            "Only single-target assignments are supported".to_string(),
        ));
    }

    let variable_name = extract_name_target(&assign.targets[0])?;
    convert_assignment_value(Some(variable_name), &assign.value)
}

fn convert_ann_assign(assign: &ast::StmtAnnAssign) -> Result<Step> {
    let Some(value) = &assign.value else {
        return Err(ReadyError::PlanParsing(
            "Annotated assignments must have a value".to_string(),
        ));
    };

    let variable_name = extract_name_target(&assign.target)?;
    convert_assignment_value(Some(variable_name), value)
}

fn convert_expression_statement(expr: &ast::StmtExpr) -> Result<Step> {
    convert_assignment_value(None, &expr.value)
}

fn convert_assignment_value(output_variable: Option<String>, value: &ast::Expr) -> Result<Step> {
    if let ast::Expr::Call(call) = value
        && let ast::Expr::Name(name) = call.func.as_ref()
    {
        let function_name = name.id.as_str();
        if function_name == "collect_user_input" {
            let prompt = call
                .args
                .first()
                .ok_or_else(|| {
                    ReadyError::PlanParsing(
                        "collect_user_input() requires a prompt argument".to_string(),
                    )
                })
                .and_then(extract_string_literal)?;

            return Ok(Step::UserInteractionStep {
                prompt,
                output_variable,
            });
        }

        let mut arguments = call
            .args
            .iter()
            .map(convert_expression)
            .collect::<Result<Vec<_>>>()?;
        for keyword in &call.keywords {
            arguments.push(convert_expression(&keyword.value)?);
        }

        return Ok(Step::ToolStep {
            tool_id: function_name.to_string(),
            arguments,
            output_variable,
        });
    }

    match output_variable {
        Some(variable_name) => Ok(Step::AssignStep {
            target: variable_name,
            value: convert_expression(value)?,
        }),
        None => Err(ReadyError::PlanParsing(format!(
            "Unsupported expression statement type: {}",
            expression_name(value)
        ))),
    }
}

fn convert_if(if_stmt: &ast::StmtIf) -> Result<Step> {
    Ok(Step::SwitchStep {
        branches: collect_if_branches(if_stmt, false)?,
    })
}

fn collect_if_branches(if_stmt: &ast::StmtIf, is_elif: bool) -> Result<Vec<ConditionalBranch>> {
    let kind = if is_elif {
        BranchKind::ElseIf
    } else {
        BranchKind::If
    };

    let mut branches = vec![ConditionalBranch {
        kind,
        condition: Some(convert_condition(&if_stmt.test)?),
        steps: convert_body(&if_stmt.body)?,
    }];

    if !if_stmt.orelse.is_empty() {
        if if_stmt.orelse.len() == 1
            && let ast::Stmt::If(next_if) = &if_stmt.orelse[0]
        {
            branches.extend(collect_if_branches(next_if, true)?);
            return Ok(branches);
        }

        branches.push(ConditionalBranch {
            kind: BranchKind::Else,
            condition: None,
            steps: convert_body(&if_stmt.orelse)?,
        });
    }

    Ok(branches)
}

fn convert_for(for_stmt: &ast::StmtFor) -> Result<Step> {
    let item_variable = extract_name_target(&for_stmt.target)?;
    let iterable_variable = match for_stmt.iter.as_ref() {
        ast::Expr::Name(name) => name.id.to_string(),
        other => {
            return Err(ReadyError::PlanParsing(format!(
                "for-loop iterable must be a simple variable name, not a '{}' — assign it to a variable first",
                expression_name(other)
            )));
        }
    };

    Ok(Step::LoopStep {
        iterable_variable,
        item_variable,
        body: convert_body(&for_stmt.body)?,
    })
}

fn convert_while(while_stmt: &ast::StmtWhile) -> Result<Step> {
    Ok(Step::WhileStep {
        condition: convert_condition(&while_stmt.test)?,
        body: convert_body(&while_stmt.body)?,
    })
}
