use std::collections::HashSet;

use super::{AbstractPlan, PrefillableInput, Step};

impl AbstractPlan {
    /// Returns top-level user interaction steps that can be satisfied ahead of runtime.
    ///
    /// Only user-interaction steps with both a non-empty prompt and an output variable
    /// are included, and nested interaction steps are intentionally ignored.
    pub fn prefillable_inputs(&self) -> Vec<PrefillableInput> {
        self.steps
            .iter()
            .filter_map(|step| match step {
                Step::UserInteractionStep {
                    prompt,
                    output_variable: Some(variable_name),
                } if !prompt.is_empty() => Some(PrefillableInput {
                    variable_name: variable_name.clone(),
                    prompt: prompt.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    /// Collects all referenced tool identifiers from the plan, including nested steps.
    ///
    /// The returned list is deduplicated and sorted to give callers a stable view of
    /// the tool dependencies required to execute the plan.
    pub fn collect_tool_ids(&self) -> Vec<String> {
        let mut ids = HashSet::new();
        collect_tool_ids_from_steps(&self.steps, &mut ids);
        let mut collected = ids.into_iter().collect::<Vec<_>>();
        collected.sort();
        collected
    }
}

fn collect_tool_ids_from_steps(steps: &[Step], ids: &mut HashSet<String>) {
    for step in steps {
        match step {
            Step::ToolStep { tool_id, .. } => {
                ids.insert(tool_id.clone());
            }
            Step::SwitchStep { branches } => {
                for branch in branches {
                    collect_tool_ids_from_steps(&branch.steps, ids);
                }
            }
            Step::LoopStep { body, .. } | Step::WhileStep { body, .. } => {
                collect_tool_ids_from_steps(body, ids);
            }
            Step::AssignStep { .. } | Step::UserInteractionStep { .. } | Step::BreakStep => {}
        }
    }
}
