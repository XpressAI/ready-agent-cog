use crate::plan::Step;

pub(crate) fn output_variable(step: &Step) -> Option<&str> {
    match step {
        Step::AssignStep { target, .. } => Some(target.as_str()),
        Step::ToolStep {
            output_variable, ..
        }
        | Step::UserInteractionStep {
            output_variable, ..
        } => output_variable.as_deref(),
        Step::SwitchStep { .. } | Step::LoopStep { .. } | Step::WhileStep { .. } | Step::BreakStep => None,
    }
}

pub(crate) fn last_output_variable(steps: &[Step]) -> Option<String> {
    steps.last().and_then(|step| match step {
        Step::AssignStep { target, .. } => Some(target.clone()),
        Step::ToolStep {
            output_variable, ..
        } => output_variable.clone(),
        Step::SwitchStep { .. }
        | Step::LoopStep { .. }
        | Step::WhileStep { .. }
        | Step::UserInteractionStep { .. }
        | Step::BreakStep => None,
    })
}
