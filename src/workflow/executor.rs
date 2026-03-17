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
}
