//! Static analysis pass that checks plans for correctness before execution.

use std::collections::HashSet;

use crate::plan::{AbstractPlan, DiagnosticSeverity, PlanDiagnostic};
use crate::tools::models::ToolDescription;

mod expressions;
mod outputs;
mod symbols;
mod tool_refs;

#[cfg(test)]
mod tests;

pub(crate) use outputs::{last_output_variable, output_variable};

pub use expressions::collect_expression_variables;
use symbols::walk_steps;

/// Validates plan structure and references, reporting errors and warnings as diagnostics.
pub fn validate_plan(
    plan: &AbstractPlan,
    available_tools: &[ToolDescription],
) -> Vec<PlanDiagnostic> {
    let mut diagnostics = Vec::new();
    let mut defined = HashSet::new();
    let mut used = HashSet::new();
    let available_tool_ids = available_tools
        .iter()
        .map(|tool| tool.id.as_str())
        .collect::<HashSet<_>>();

    walk_steps(
        &plan.steps,
        &mut defined,
        &mut used,
        &mut diagnostics,
        &available_tool_ids,
    );

    let last_output = last_output_variable(&plan.steps);
    for step in &plan.steps {
        if let Some(output) = output_variable(step)
            && !used.contains(output)
            && Some(output) != last_output.as_deref()
        {
            diagnostics.push(PlanDiagnostic {
                severity: DiagnosticSeverity::Warning,
                message: format!("Warning: local variable '{output}' is assigned to but never used"),
                variable_name: Some(output.to_string()),
            });
        }
    }

    diagnostics
}
