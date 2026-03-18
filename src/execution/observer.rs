//! Execution lifecycle observer trait and built-in implementations.

use tracing::{debug, error, info};

use crate::error::ReadyError;
use crate::execution::state::{ExecutionState, StepResult};
use crate::plan::Step;

/// Receives lifecycle callbacks as a plan starts, advances, suspends, errors, and completes.
pub trait ExecutionObserver: Send + Sync {
    /// Called before plan execution begins.
    fn on_plan_start(&self, plan_name: &str, state: &ExecutionState);
    /// Called immediately before an individual step is executed.
    fn on_step_start(&self, step_index: usize, step: &Step, ip: &[usize], state: &ExecutionState);
    /// Called after a step completes successfully.
    fn on_step_complete(&self, step_index: usize, step: &Step, result: &StepResult);
    /// Called when execution suspends and awaits external input.
    fn on_suspension(&self, reason: &str, state: &ExecutionState);
    /// Called when step execution produces an error.
    fn on_error(&self, step: &Step, error: &ReadyError, state: &ExecutionState);
    /// Called after execution reaches a terminal state.
    fn on_plan_complete(&self, state: &ExecutionState);
}

/// No-op observer that ignores all execution lifecycle events.
pub struct NoOpObserver;

impl ExecutionObserver for NoOpObserver {
    fn on_plan_start(&self, _plan_name: &str, _state: &ExecutionState) {}

    fn on_step_start(
        &self,
        _step_index: usize,
        _step: &Step,
        _ip: &[usize],
        _state: &ExecutionState,
    ) {
    }

    fn on_step_complete(&self, _step_index: usize, _step: &Step, _result: &StepResult) {}

    fn on_suspension(&self, _reason: &str, _state: &ExecutionState) {}

    fn on_error(&self, _step: &Step, _error: &ReadyError, _state: &ExecutionState) {}

    fn on_plan_complete(&self, _state: &ExecutionState) {}
}

/// Observer that logs execution lifecycle events with [`tracing::info!`](src/execution/observer.rs:43).
pub struct LoggingObserver;

impl ExecutionObserver for LoggingObserver {
    fn on_plan_start(&self, plan_name: &str, state: &ExecutionState) {
        info!(
            plan_name = plan_name,
            current_step_index = state.current_step_index.unwrap_or(0),
            "plan started"
        );
    }

    fn on_step_start(&self, step_index: usize, step: &Step, ip: &[usize], _state: &ExecutionState) {
        info!(
            step_index = step_index,
            ip = ?ip,
            step = %step_type_name(step),
            details = %step_details(step),
            "step started"
        );
    }

    fn on_step_complete(&self, step_index: usize, step: &Step, result: &StepResult) {
        info!(
            step_index = step_index,
            step = %step_type_name(step),
            suspend = result.suspend,
            suspend_reason = ?result.suspend_reason,
            "step completed"
        );
    }

    fn on_suspension(&self, reason: &str, _state: &ExecutionState) {
        info!(reason = reason, "execution suspended");
    }

    fn on_error(&self, step: &Step, error: &ReadyError, _state: &ExecutionState) {
        error!(step = %step_type_name(step), error = %error, "execution error");
    }

    fn on_plan_complete(&self, state: &ExecutionState) {
        info!(
            status = ?state.status,
            variables = state.interpreter_state.variables.len(),
            "plan completed"
        );
        debug!(
            ip_path = ?state.interpreter_state.ip_path,
            variables = ?state.interpreter_state.variables,
            pending_input_variable = ?state.interpreter_state.pending_input_variable,
            pending_tool_id = ?state.interpreter_state.pending_tool_id,
            pending_tool_state = ?state.interpreter_state.pending_tool_state,
            pending_resume_value = ?state.interpreter_state.pending_resume_value,
            suspension_reason = ?state.suspension_reason,
            "final execution state snapshot"
        );
    }
}

pub(crate) fn step_type_name(step: &Step) -> &'static str {
    match step {
        Step::AssignStep { .. } => "AssignStep",
        Step::ToolStep { .. } => "ToolStep",
        Step::SwitchStep { .. } => "SwitchStep",
        Step::LoopStep { .. } => "LoopStep",
        Step::WhileStep { .. } => "WhileStep",
        Step::UserInteractionStep { .. } => "UserInteractionStep",
    }
}

fn step_details(step: &Step) -> String {
    match step {
        Step::ToolStep {
            tool_id,
            output_variable,
            ..
        } => format!("tool_id={tool_id:?} output_variable={output_variable:?}"),
        _ => String::new(),
    }
}
