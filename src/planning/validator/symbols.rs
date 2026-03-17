use std::collections::HashSet;

use crate::plan::{DiagnosticSeverity, PlanDiagnostic, Step};

use super::expressions::collect_expression_variables;
use super::outputs::output_variable;
use super::tool_refs::validate_tool_reference;

pub(crate) fn walk_steps(
    steps: &[Step],
    defined: &mut HashSet<String>,
    used: &mut HashSet<String>,
    diagnostics: &mut Vec<PlanDiagnostic>,
    available_tool_ids: &HashSet<&str>,
) {
    for step in steps {
        check_step_references(step, defined, used, diagnostics, available_tool_ids);

        if let Some(output) = output_variable(step) {
            defined.insert(output.to_string());
        }

        match step {
            Step::SwitchStep { branches } => {
                for branch in branches {
                    let mut branch_defined = defined.clone();
                    walk_steps(
                        &branch.steps,
                        &mut branch_defined,
                        used,
                        diagnostics,
                        available_tool_ids,
                    );
                }
            }
            Step::LoopStep {
                item_variable,
                body,
                ..
            } => {
                let mut loop_defined = defined.clone();
                loop_defined.insert(item_variable.clone());
                walk_steps(
                    body,
                    &mut loop_defined,
                    used,
                    diagnostics,
                    available_tool_ids,
                );
            }
            Step::WhileStep { body, .. } => {
                let mut while_defined = defined.clone();
                walk_steps(
                    body,
                    &mut while_defined,
                    used,
                    diagnostics,
                    available_tool_ids,
                );
            }
            Step::AssignStep { .. } | Step::ToolStep { .. } | Step::UserInteractionStep { .. } => {}
        }
    }
}

fn check_step_references(
    step: &Step,
    defined: &HashSet<String>,
    used: &mut HashSet<String>,
    diagnostics: &mut Vec<PlanDiagnostic>,
    available_tool_ids: &HashSet<&str>,
) {
    validate_tool_reference(step, diagnostics, available_tool_ids);

    if let Step::LoopStep {
        iterable_variable, ..
    } = step
    {
        used.insert(iterable_variable.clone());
        if !defined.contains(iterable_variable) {
            diagnostics.push(PlanDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: format!("Variable '{iterable_variable}' is not defined before it is used"),
                variable_name: Some(iterable_variable.clone()),
            });
        }
    }

    for variable in referenced_variables(step) {
        used.insert(variable.clone());
        if !defined.contains(&variable) {
            diagnostics.push(PlanDiagnostic {
                severity: DiagnosticSeverity::Error,
                message: format!("Variable '{variable}' is not defined before it is used"),
                variable_name: Some(variable),
            });
        }
    }
}

fn referenced_variables(step: &Step) -> HashSet<String> {
    match step {
        Step::ToolStep { arguments, .. } => arguments
            .iter()
            .flat_map(collect_expression_variables)
            .collect(),
        Step::AssignStep { value, .. } => collect_expression_variables(value),
        Step::SwitchStep { branches } => branches
            .iter()
            .filter_map(|branch| branch.condition.as_ref())
            .flat_map(collect_expression_variables)
            .collect(),
        Step::WhileStep { condition, .. } => collect_expression_variables(condition),
        Step::LoopStep { .. } | Step::UserInteractionStep { .. } => HashSet::new(),
    }
}
