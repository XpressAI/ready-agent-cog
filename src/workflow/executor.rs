//! High-level plan executor that resolves, configures, and runs an [`AbstractPlan`](src/plan.rs:1).

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::Result;
use crate::execution::interpreter::PlanInterpreter;
use crate::execution::observer::{ExecutionObserver, NoOpObserver};
use crate::execution::state::{ExecutionState, ExecutionStatus};
use crate::plan::AbstractPlan;
use crate::tools::models::ToolDescription;
use crate::tools::registry::InMemoryToolRegistry;

type SuspendCallback = dyn Fn(&str) -> Option<String> + Send + Sync;

#[async_trait]
pub(crate) trait PlanRunner: Send + Sync {
    async fn execute(
        &self,
        registry: Arc<InMemoryToolRegistry>,
        observer: Arc<dyn ExecutionObserver>,
        plan: &AbstractPlan,
        state: &mut ExecutionState,
    ) -> Result<()>;

    async fn provide_input(
        &self,
        registry: Arc<InMemoryToolRegistry>,
        observer: Arc<dyn ExecutionObserver>,
        plan: &AbstractPlan,
        state: &mut ExecutionState,
        value: Value,
    ) -> Result<()>;
}

struct InterpreterPlanRunner;

#[async_trait]
impl PlanRunner for InterpreterPlanRunner {
    async fn execute(
        &self,
        registry: Arc<InMemoryToolRegistry>,
        observer: Arc<dyn ExecutionObserver>,
        plan: &AbstractPlan,
        state: &mut ExecutionState,
    ) -> Result<()> {
        PlanInterpreter::new(registry, plan.clone())
            .with_observer(observer)
            .execute(state)
            .await
    }

    async fn provide_input(
        &self,
        registry: Arc<InMemoryToolRegistry>,
        observer: Arc<dyn ExecutionObserver>,
        plan: &AbstractPlan,
        state: &mut ExecutionState,
        value: Value,
    ) -> Result<()> {
        PlanInterpreter::new(registry, plan.clone())
            .with_observer(observer)
            .provide_input(state, value)
            .await
    }
}

/// High-level executor that resolves a plan against the tool registry and runs it through a [`PlanInterpreter`](src/execution/interpreter.rs:1).
/// It also manages optional observer wiring and pre-filled execution inputs.
pub struct SopExecutor {
    registry: Arc<InMemoryToolRegistry>,
    observer: Arc<dyn ExecutionObserver>,
    runner: Arc<dyn PlanRunner>,
}

impl SopExecutor {
    /// Constructs an executor with a tool registry and an optional [`ExecutionObserver`](src/execution/observer.rs:1).
    pub fn new(
        registry: Arc<InMemoryToolRegistry>,
        observer: Option<Arc<dyn ExecutionObserver>>,
    ) -> Self {
        Self::with_runner(registry, observer, Arc::new(InterpreterPlanRunner))
    }

    pub(crate) fn with_runner(
        registry: Arc<InMemoryToolRegistry>,
        observer: Option<Arc<dyn ExecutionObserver>>,
        runner: Arc<dyn PlanRunner>,
    ) -> Self {
        Self {
            registry,
            observer: observer.unwrap_or_else(|| Arc::new(NoOpObserver)),
            runner,
        }
    }

    /// Executes a plan with optional pre-filled inputs and optional suspension handling.
    /// Returns the final [`ExecutionState`](src/execution/state.rs:1) after completion or suspension.
    pub async fn execute(
        &self,
        plan: &AbstractPlan,
        initial_inputs: HashMap<String, Value>,
        on_suspend: Option<Box<SuspendCallback>>,
    ) -> Result<ExecutionState> {
        let mut state = ExecutionState {
            status: ExecutionStatus::Running,
            ..Default::default()
        };
        state.interpreter_state.variables.extend(initial_inputs);

        self.runner
            .execute(
                self.registry.clone(),
                self.observer.clone(),
                plan,
                &mut state,
            )
            .await?;

        while state.status == ExecutionStatus::Suspended {
            let Some(callback) = on_suspend.as_ref() else {
                break;
            };

            let reason = state
                .suspension_reason
                .as_deref()
                .unwrap_or("User input required");

            let Some(value) = callback(reason) else {
                break;
            };

            self.runner
                .provide_input(
                    self.registry.clone(),
                    self.observer.clone(),
                    plan,
                    &mut state,
                    Value::String(value),
                )
                .await?;
        }

        Ok(state)
    }

    /// Resumes a suspended execution by providing the next user input value.
    /// This continues the underlying interpreter from the pending interaction point.
    pub async fn resume(
        &self,
        plan: &AbstractPlan,
        state: &mut ExecutionState,
        value: Value,
    ) -> Result<()> {
        self.runner
            .provide_input(
                self.registry.clone(),
                self.observer.clone(),
                plan,
                state,
                value,
            )
            .await
    }

    /// Executes a plan with automatic error recovery.
    ///
    /// On execution error, if the error is recoverable and recovery attempts remain,
    /// this method calls the planner to generate a recovery plan and executes it
    /// from the error checkpoint.
    pub async fn execute_with_recovery(
        &self,
        plan: &AbstractPlan,
        initial_inputs: HashMap<String, Value>,
        on_suspend: Option<Box<SuspendCallback>>,
        planner: &dyn RecoveryPlanner,
        sop_text: &str,
        tool_descriptions: &[ToolDescription],
        max_recovery_attempts: usize,
    ) -> Result<ExecutionState> {
        let mut state = self.execute(plan, initial_inputs, on_suspend).await?;

        let mut recovery_attempts = 0;
        while state.status == ExecutionStatus::Failed && recovery_attempts < max_recovery_attempts {
            let Some(error) = state.error.clone() else {
                break;
            };

            // Execution errors are always recoverable by definition.
            // The error type is already known to be Execution since it comes from ExecutionState.
            // We proceed with recovery for all execution errors.
            recovery_attempts += 1;

            // Generate recovery context
            let recovery_context = crate::execution::state::RecoveryContext::new(
                plan.clone(),
                state.clone(),
                error,
            );

            // Generate recovery plan
            let recovery_plan = planner
                .recover(sop_text, &recovery_context, tool_descriptions)
                .await?;

            // Execute recovery plan from checkpoint (no suspension handling during recovery)
            state = self
                .execute_from_checkpoint(&recovery_plan, &state, None)
                .await?;
        }

        Ok(state)
    }

    /// Executes a plan from a checkpoint state.
    ///
    /// This method initializes execution from an existing state (e.g., after an error),
    /// preserving variables and position from the checkpoint.
    async fn execute_from_checkpoint(
        &self,
        plan: &AbstractPlan,
        checkpoint: &ExecutionState,
        on_suspend: Option<Box<SuspendCallback>>,
    ) -> Result<ExecutionState> {
        // Start from checkpoint state
        let mut state = checkpoint.clone();
        state.status = ExecutionStatus::Running;
        state.error = None;

        // Execute the recovery plan
        self.runner
            .execute(
                self.registry.clone(),
                self.observer.clone(),
                plan,
                &mut state,
            )
            .await?;

        // Handle suspension if needed
        while state.status == ExecutionStatus::Suspended {
            let Some(callback) = on_suspend.as_ref() else {
                break;
            };

            let reason = state
                .suspension_reason
                .as_deref()
                .unwrap_or("User input required");

            let Some(value) = callback(reason) else {
                break;
            };

            self.runner
                .provide_input(
                    self.registry.clone(),
                    self.observer.clone(),
                    plan,
                    &mut state,
                    Value::String(value),
                )
                .await?;
        }

        Ok(state)
    }
}

/// Trait for recovery planning, allowing mock implementations in tests.
#[async_trait]
pub trait RecoveryPlanner: Send + Sync {
    async fn recover(
        &self,
        sop_text: &str,
        recovery_context: &crate::execution::state::RecoveryContext,
        tool_descriptions: &[ToolDescription],
    ) -> Result<AbstractPlan>;
}
