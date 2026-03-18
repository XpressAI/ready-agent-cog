//! The core plan interpreter that walks plan steps using an instruction pointer.

use std::sync::Arc;

use serde_json::Value;

use crate::error::{ReadyError, Result};
use crate::execution::evaluator::evaluate_expression;
use crate::execution::navigator::{on_scope_exit, resolve_step, select_branch, step_at_path};
use crate::execution::observer::{ExecutionObserver, NoOpObserver};
use crate::execution::state::{
    ExecutionError, ExecutionState, ExecutionStatus, InstructionPointer, InternalState,
    InterpreterState, LoopState, StepResult, WhileState,
};
use crate::plan::{AbstractPlan, Step};
use crate::tools::models::{Continuation, ToolCall, ToolResult};
use crate::tools::registry::InMemoryToolRegistry;

/// Core execution engine that walks plan steps, dispatches tool calls, and manages runtime state.
pub struct PlanInterpreter {
    plan: AbstractPlan,
    registry: Arc<InMemoryToolRegistry>,
    observer: Arc<dyn ExecutionObserver>,
    max_while_iterations: usize,
}

impl PlanInterpreter {
    /// Constructs an interpreter from a tool registry and an abstract plan.
    pub fn new(registry: Arc<InMemoryToolRegistry>, plan: AbstractPlan) -> Self {
        Self {
            plan,
            registry,
            observer: Arc::new(NoOpObserver),
            max_while_iterations: 1000,
        }
    }

    /// Attaches an [`ExecutionObserver`](src/execution/observer.rs) for execution lifecycle hooks.
    pub fn with_observer(mut self, observer: Arc<dyn ExecutionObserver>) -> Self {
        self.observer = observer;
        self
    }

    /// Sets the maximum number of iterations allowed for any `while` loop.
    pub fn with_max_while_iterations(mut self, max_while_iterations: usize) -> Self {
        self.max_while_iterations = max_while_iterations;
        self
    }

    /// Executes the plan from the current state until it completes, fails, or suspends.
    /// Updates the provided [`ExecutionState`](src/execution/state.rs) in place as execution progresses.
    pub async fn execute(&self, state: &mut ExecutionState) -> Result<()> {
        state.status = ExecutionStatus::Running;
        state.error = None;
        state.suspension_reason = None;

        let mut internal_state = InternalState::default();
        self.observer.on_plan_start(&self.plan.name, state);

        let result = self.run_loop(state, &mut internal_state).await;

        match result {
            Ok(()) => {
                if state.status == ExecutionStatus::Running {
                    state.status = ExecutionStatus::Completed;
                }
            }
            Err(error) => {
                if state.status != ExecutionStatus::Suspended {
                    state.status = ExecutionStatus::Failed;
                    state.error = Some(ExecutionError::from_step(
                        state.current_step_index,
                        None,
                        error.to_string(),
                        error.to_string(),
                    ));
                }
                self.observer.on_plan_complete(state);
                return Err(error);
            }
        }

        self.observer.on_plan_complete(state);
        Ok(())
    }

    /// Resumes a suspended execution by providing input and continuing the plan.
    /// Input is routed either to a waiting user interaction step or a suspended tool continuation.
    pub async fn provide_input(&self, state: &mut ExecutionState, value: Value) -> Result<()> {
        let interpreter_state = &mut state.interpreter_state;

        if interpreter_state.pending_tool_id.is_some() {
            interpreter_state.pending_resume_value = Some(value);
            interpreter_state.pending_input_variable = None;
        } else {
            if let Some(output_var) = interpreter_state.pending_input_variable.take() {
                interpreter_state.variables.insert(output_var, value);
            }

            let mut ip = instruction_pointer(interpreter_state)?;
            ip.advance();
            interpreter_state.ip_path = ip.path;
        }

        self.execute(state).await
    }

    async fn run_loop(
        &self,
        state: &mut ExecutionState,
        internal_state: &mut InternalState,
    ) -> Result<()> {
        let mut step_counter = 0usize;

        loop {
            let mut ip = instruction_pointer(&state.interpreter_state)?;
            let resolution = resolve_step(&self.plan.steps, &ip, internal_state);

            match resolution {
                None => break,
                Some((None, _)) => {
                    if ip.depth() <= 1 {
                        break;
                    }

                    let parent_path = ip.path[..ip.path.len() - 1].to_vec();
                    let parent_step = step_at_path(&self.plan.steps, &parent_path, internal_state);
                    on_scope_exit(parent_step, internal_state, &mut ip, &parent_path);
                    state.interpreter_state.ip_path = ip.path;
                }
                Some((Some(step), _)) => {
                    state.current_step_index = Some(step_counter);
                    step_counter += 1;

                    self.observer.on_step_start(
                        state.current_step_index.unwrap_or_default(),
                        step,
                        &ip.path,
                        state,
                    );

                    let result = self
                        .dispatch_step(step, &mut ip, state, internal_state)
                        .await;
                    match result {
                        Ok(step_result) => {
                            self.observer.on_step_complete(
                                state.current_step_index.unwrap_or_default(),
                                step,
                                &step_result,
                            );
                            state.interpreter_state.ip_path = ip.path;

                            if step_result.suspend {
                                state.status = ExecutionStatus::Suspended;
                                state.suspension_reason = step_result.suspend_reason.clone();
                                self.observer.on_suspension(
                                    step_result.suspend_reason.as_deref().unwrap_or_default(),
                                    state,
                                );
                                return Ok(());
                            }
                        }
                        Err(error) => {
                            self.observer.on_error(step, &error, state);
                            return Err(error);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn dispatch_step(
        &self,
        step: &Step,
        ip: &mut InstructionPointer,
        state: &mut ExecutionState,
        internal_state: &mut InternalState,
    ) -> Result<StepResult> {
        match step {
            Step::AssignStep { target, value } => handle_assign_step(target, value, ip, state),
            Step::ToolStep {
                tool_id,
                arguments,
                output_variable,
            } => {
                self.execute_tool_step(tool_id, arguments, output_variable.as_ref(), ip, state)
                    .await
            }
            Step::SwitchStep { branches } => handle_switch_step(
                step,
                branches,
                ip,
                &state.interpreter_state.variables,
                internal_state,
            ),
            Step::LoopStep {
                iterable_variable,
                item_variable,
                ..
            } => handle_loop_step(
                iterable_variable,
                item_variable,
                ip,
                state.current_step_index,
                &mut state.interpreter_state.variables,
                internal_state,
            ),
            Step::WhileStep { condition, .. } => handle_while_step(
                condition,
                ip,
                state.current_step_index,
                &state.interpreter_state.variables,
                internal_state,
                self.max_while_iterations,
            ),
            Step::UserInteractionStep {
                prompt,
                output_variable,
            } => handle_user_interaction_step(
                prompt,
                output_variable,
                ip,
                &mut state.interpreter_state,
            ),
            Step::BreakStep => handle_break_step(&self.plan.steps, internal_state, ip),
        }
    }

    async fn execute_tool_step(
        &self,
        tool_id: &str,
        arguments: &[crate::plan::Expression],
        output_variable: Option<&String>,
        ip: &mut InstructionPointer,
        state: &mut ExecutionState,
    ) -> Result<StepResult> {
        let call = {
            let interpreter_state = &mut state.interpreter_state;
            if interpreter_state.pending_tool_id.as_deref() == Some(tool_id) {
                let state = interpreter_state.pending_tool_state.take();
                let resume_value = interpreter_state.pending_resume_value.take();
                interpreter_state.pending_tool_id = None;
                ToolCall {
                    tool_id: tool_id.to_string(),
                    args: Vec::new(),
                    continuation: Some(Continuation {
                        state: state.unwrap_or(serde_json::Value::Null),
                        resume_value,
                    }),
                }
            } else {
                let args = arguments
                    .iter()
                    .map(|argument| evaluate_expression(argument, &interpreter_state.variables))
                    .collect::<Result<Vec<_>>>()?;
                ToolCall {
                    tool_id: tool_id.to_string(),
                    args,
                    continuation: None,
                }
            }
        };

        let module = self
            .registry
            .get_module_for_tool(tool_id)
            .ok_or_else(|| ReadyError::ToolNotFound(tool_id.to_string()))?;

        let result = module.execute(&call).await?;

        match result {
            ToolResult::Success(value) => {
                if let Some(output_variable) = output_variable {
                    state
                        .interpreter_state
                        .variables
                        .insert(output_variable.clone(), value);
                }
                ip.advance();
                Ok(StepResult::default())
            }
            ToolResult::Suspended(suspension) => {
                state.interpreter_state.pending_tool_id = Some(tool_id.to_string());
                state.interpreter_state.pending_tool_state = Some(suspension.continuation_state);
                Ok(StepResult {
                    suspend: true,
                    suspend_reason: Some(suspension.reason),
                })
            }
        }
    }
}

pub(crate) fn handle_assign_step(
    target: &str,
    value: &crate::plan::Expression,
    ip: &mut InstructionPointer,
    state: &mut ExecutionState,
) -> Result<StepResult> {
    let value = evaluate_expression(value, &state.interpreter_state.variables)?;
    state
        .interpreter_state
        .variables
        .insert(target.to_string(), value);
    ip.advance();
    Ok(StepResult::default())
}

pub(crate) fn handle_switch_step(
    step: &Step,
    branches: &[crate::plan::ConditionalBranch],
    ip: &mut InstructionPointer,
    variables: &std::collections::HashMap<String, Value>,
    internal_state: &mut InternalState,
) -> Result<StepResult> {
    let branch_index = select_branch(step, variables);
    if let Some(branch_index) = branch_index {
        internal_state
            .branches
            .insert(ip.path.to_vec(), branch_index);
        let has_steps = branches
            .get(branch_index)
            .is_some_and(|branch| !branch.steps.is_empty());
        if has_steps {
            ip.descend();
        } else {
            internal_state.branches.remove(&ip.path.to_vec());
            ip.advance();
        }
    } else {
        ip.advance();
    }
    Ok(StepResult::default())
}

pub(crate) fn handle_loop_step(
    iterable_variable: &str,
    item_variable: &str,
    ip: &mut InstructionPointer,
    step_index: Option<usize>,
    variables: &mut std::collections::HashMap<String, Value>,
    internal_state: &mut InternalState,
) -> Result<StepResult> {
    let key = ip.path.to_vec();
    if let Some(loop_state) = internal_state.loops.get_mut(&key) {
        loop_state.index += 1;
        if loop_state.index < loop_state.items.len() {
            variables.insert(
                item_variable.to_string(),
                loop_state.items[loop_state.index].clone(),
            );
            ip.descend();
        } else {
            internal_state.loops.remove(&key);
            ip.advance();
        }
        return Ok(StepResult::default());
    }

    let iterable =
        variables
            .get(iterable_variable)
            .cloned()
            .ok_or_else(|| ReadyError::Execution {
                step_index,
                step_type: Some("LoopStep".to_string()),
                message: format!("Undefined iterable variable: '{iterable_variable}'"),
            })?;

    let Value::Array(items) = iterable else {
        return Err(ReadyError::Execution {
            step_index,
            step_type: Some("LoopStep".to_string()),
            message: format!("Loop variable '{iterable_variable}' is not iterable"),
        });
    };

    if items.is_empty() {
        ip.advance();
        return Ok(StepResult::default());
    }

    variables.insert(item_variable.to_string(), items[0].clone());
    internal_state
        .loops
        .insert(key, LoopState { index: 0, items });
    ip.descend();
    Ok(StepResult::default())
}

pub(crate) fn handle_while_step(
    condition: &crate::plan::Expression,
    ip: &mut InstructionPointer,
    step_index: Option<usize>,
    variables: &std::collections::HashMap<String, Value>,
    internal_state: &mut InternalState,
    max_while_iterations: usize,
) -> Result<StepResult> {
    let key = ip.path.to_vec();
    if let Some(while_state) = internal_state.whiles.get_mut(&key) {
        while_state.iterations += 1;
        if while_state.iterations > max_while_iterations {
            internal_state.whiles.remove(&key);
            return Err(ReadyError::Execution {
                step_index,
                step_type: Some("WhileStep".to_string()),
                message: format!(
                    "WhileStep exceeded maximum iterations ({})",
                    max_while_iterations
                ),
            });
        }

        let condition_value = evaluate_expression(condition, variables)?;
        if crate::execution::evaluator::is_truthy(&condition_value) {
            ip.descend();
        } else {
            internal_state.whiles.remove(&key);
            ip.advance();
        }
        return Ok(StepResult::default());
    }

    let condition_value = evaluate_expression(condition, variables)?;
    if crate::execution::evaluator::is_truthy(&condition_value) {
        internal_state
            .whiles
            .insert(key, WhileState { iterations: 1 });
        ip.descend();
    } else {
        ip.advance();
    }
    Ok(StepResult::default())
}

pub(crate) fn handle_user_interaction_step(
    prompt: &str,
    output_variable: &Option<String>,
    ip: &mut InstructionPointer,
    interpreter_state: &mut InterpreterState,
) -> Result<StepResult> {
    if let Some(output_variable) = output_variable
        && interpreter_state.variables.contains_key(output_variable)
    {
        ip.advance();
        return Ok(StepResult::default());
    }

    interpreter_state.pending_input_variable = output_variable.clone();
    Ok(StepResult {
        suspend: true,
        suspend_reason: Some(prompt.to_string()),
    })
}

pub(crate) fn handle_break_step(
    plan_steps: &[Step],
    internal_state: &mut InternalState,
    ip: &mut InstructionPointer,
) -> Result<StepResult> {
    // Walk up the path to find the nearest enclosing loop (LoopStep or WhileStep).
    // For each level, clean up any switch branch state, then advance past the loop.
    for depth in (0..ip.path.len().saturating_sub(1)).rev() {
        let ancestor_path = &ip.path[..=depth];
        let ancestor = step_at_path(plan_steps, ancestor_path, internal_state);
        if matches!(ancestor, Some(Step::LoopStep { .. }) | Some(Step::WhileStep { .. })) {
            let loop_path = ancestor_path.to_vec();
            internal_state.loops.remove(&loop_path);
            internal_state.whiles.remove(&loop_path);
            // Clean up any switch branch state for scopes between break and the loop.
            for inner_depth in (depth + 1)..ip.path.len().saturating_sub(1) {
                internal_state.branches.remove(&ip.path[..=inner_depth].to_vec());
            }
            ip.path = loop_path;
            ip.advance();
            return Ok(StepResult::default());
        }
    }
    Err(ReadyError::PlanParsing(
        "break statement used outside of a loop".to_string(),
    ))
}

fn instruction_pointer(state: &InterpreterState) -> Result<InstructionPointer> {
    InstructionPointer::try_from(state.ip_path.clone()).map_err(|message| ReadyError::Execution {
        step_index: None,
        step_type: None,
        message: message.to_string(),
    })
}
