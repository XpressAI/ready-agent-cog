//! Step navigation within the plan tree using instruction pointer paths.

use crate::execution::evaluator::{evaluate_expression, is_truthy};
use crate::execution::state::{InstructionPointer, InternalState};
use crate::plan::Step;

/// Resolves the current step for an instruction pointer within nested plan containers.
/// Returns the matched step and the step slice at the current depth, or `None` when execution is past the plan.
pub fn resolve_step<'a>(
    plan_steps: &'a [Step],
    ip: &InstructionPointer,
    state: &InternalState,
) -> Option<(Option<&'a Step>, &'a [Step])> {
    let mut steps = plan_steps;
    let depth = ip.depth();

    for level in 0..depth {
        let idx = ip.path[level];

        if idx >= steps.len() {
            if level == 0 {
                return None;
            }
            return Some((None, steps));
        }

        if level == depth - 1 {
            return Some((Some(&steps[idx]), steps));
        }

        let container = &steps[idx];
        steps = get_child_steps(container, &ip.path[..=level], state);
    }

    None
}

/// Looks up a step directly by path within a nested step list.
/// Returns `None` if any index in the path is out of range.
pub fn step_at_path<'a>(
    plan_steps: &'a [Step],
    path: &[usize],
    state: &InternalState,
) -> Option<&'a Step> {
    let mut steps = plan_steps;

    for depth in 0..path.len() {
        let idx = path[depth];
        if idx >= steps.len() {
            return None;
        }

        if depth == path.len() - 1 {
            return Some(&steps[idx]);
        }

        let container = &steps[idx];
        steps = get_child_steps(container, &path[..=depth], state);
    }

    None
}

/// Returns the child steps for a container step at the given path.
/// Loop and while steps return their bodies, while switch steps return the currently selected branch.
pub fn get_child_steps<'a>(
    container: &'a Step,
    path_prefix: &[usize],
    state: &InternalState,
) -> &'a [Step] {
    match container {
        Step::SwitchStep { branches } => {
            let key = path_prefix.to_vec();
            let Some(&branch_index) = state.branches.get(&key) else {
                return &[];
            };

            branches
                .get(branch_index)
                .map(|branch| branch.steps.as_slice())
                .unwrap_or(&[])
        }
        Step::LoopStep { body, .. } | Step::WhileStep { body, .. } => body.as_slice(),
        other => panic!("IP descends into non-container step: {other:?}"),
    }
}

/// Performs cleanup when execution exits a container scope.
/// This updates the instruction pointer and clears any transient branch selection state as needed.
pub fn on_scope_exit(
    parent_step: Option<&Step>,
    state: &mut InternalState,
    ip: &mut InstructionPointer,
    parent_path: &[usize],
) {
    match parent_step {
        Some(Step::LoopStep { .. }) | Some(Step::WhileStep { .. }) => {
            ip.path.truncate(parent_path.len());
            ip.path.clone_from_slice(parent_path);
        }
        Some(Step::SwitchStep { .. }) => {
            let key = parent_path.to_vec();
            state.branches.remove(&key);
            ip.ascend();
        }
        _ => ip.ascend(),
    }
}

/// Selects the index of the first matching branch for a switch step.
/// Branch conditions are evaluated against the current variable scope using runtime truthiness rules.
pub fn select_branch(
    step: &Step,
    variables: &std::collections::HashMap<String, serde_json::Value>,
) -> Option<usize> {
    let Step::SwitchStep { branches } = step else {
        return None;
    };

    for (index, branch) in branches.iter().enumerate() {
        match &branch.condition {
            None => return Some(index),
            Some(condition) => {
                let value = evaluate_expression(condition, variables).ok()?;
                if is_truthy(&value) {
                    return Some(index);
                }
            }
        }
    }

    None
}
