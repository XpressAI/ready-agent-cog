//! Process-backed tools that expose saved abstract plans as callable sub-plan tools.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::error::{ReadyError, Result};
use crate::execution::interpreter::PlanInterpreter;
use crate::execution::state::{ExecutionState, ExecutionStatus};
use crate::plan::AbstractPlan;
use crate::tools::models::{
    ToolArgumentDescription, ToolCall, ToolDescription, ToolResult, ToolReturnDescription,
    ToolSuspension,
};
use crate::tools::registry::InMemoryToolRegistry;
use crate::tools::traits::ToolsModule;

/// Wraps saved plans so they can be invoked as tools through nested interpretation.
pub struct ProcessToolsModule {
    plans: HashMap<String, AbstractPlan>,
    descriptions: Vec<ToolDescription>,
    registry: Arc<InMemoryToolRegistry>,
    runner: Arc<dyn ProcessPlanRunner>,
}

#[async_trait]
pub(crate) trait ProcessPlanRunner: Send + Sync {
    async fn execute(
        &self,
        registry: Arc<InMemoryToolRegistry>,
        plan: &AbstractPlan,
        state: &mut ExecutionState,
    ) -> Result<()>;

    async fn provide_input(
        &self,
        registry: Arc<InMemoryToolRegistry>,
        plan: &AbstractPlan,
        state: &mut ExecutionState,
        value: Value,
    ) -> Result<()>;
}

struct InterpreterProcessPlanRunner;

#[async_trait]
impl ProcessPlanRunner for InterpreterProcessPlanRunner {
    async fn execute(
        &self,
        registry: Arc<InMemoryToolRegistry>,
        plan: &AbstractPlan,
        state: &mut ExecutionState,
    ) -> Result<()> {
        PlanInterpreter::new(registry, plan.clone())
            .execute(state)
            .await
    }

    async fn provide_input(
        &self,
        registry: Arc<InMemoryToolRegistry>,
        plan: &AbstractPlan,
        state: &mut ExecutionState,
        value: Value,
    ) -> Result<()> {
        PlanInterpreter::new(registry, plan.clone())
            .provide_input(state, value)
            .await
    }
}

impl ProcessToolsModule {
    /// Constructs a process tools module and validates referenced tools up front.
    pub fn new(
        plans: HashMap<String, AbstractPlan>,
        registry: InMemoryToolRegistry,
    ) -> Result<Self> {
        Self::with_runner(plans, registry, Arc::new(InterpreterProcessPlanRunner))
    }

    pub(crate) fn with_runner(
        plans: HashMap<String, AbstractPlan>,
        registry: InMemoryToolRegistry,
        runner: Arc<dyn ProcessPlanRunner>,
    ) -> Result<Self> {
        validate_process_plans(&plans, &registry)?;
        let registry = Arc::new(registry);

        let descriptions = build_process_descriptions(&plans);

        Ok(Self {
            plans,
            descriptions,
            registry,
            runner,
        })
    }
}

pub(crate) fn build_process_descriptions(
    plans: &HashMap<String, AbstractPlan>,
) -> Vec<ToolDescription> {
    plans
        .values()
        .map(|plan| ToolDescription {
            id: plan.name.clone(),
            description: plan.description.clone(),
            arguments: plan
                .prefillable_inputs()
                .into_iter()
                .map(|input| ToolArgumentDescription {
                    name: input.variable_name,
                    description: input.prompt,
                    type_name: "str".to_string(),
                    default: None,
                })
                .collect(),
            returns: ToolReturnDescription {
                name: None,
                description: String::new(),
                type_name: None,
                fields: Vec::new(),
            },
        })
        .collect()
}

pub(crate) fn seed_execution_state(
    descriptions: &[ToolDescription],
    tool_id: &str,
    args: &[Value],
) -> Result<ExecutionState> {
    let description = descriptions
        .iter()
        .find(|d| d.id == tool_id)
        .ok_or_else(|| ReadyError::ToolNotFound(tool_id.to_string()))?;

    let mut state = ExecutionState::default();
    for (index, argument) in description.arguments.iter().enumerate() {
        if let Some(value) = args.get(index) {
            state
                .interpreter_state
                .variables
                .insert(argument.name.clone(), value.clone());
        }
    }
    Ok(state)
}

pub(crate) fn restore_execution_state(call: &ToolCall) -> Result<ExecutionState> {
    match &call.continuation {
        Some(cont) => Ok(serde_json::from_value::<ExecutionState>(
            cont.state.clone(),
        )?),
        None => Err(ReadyError::Tool {
            tool_id: call.tool_id.clone(),
            message: "Missing continuation state".to_string(),
        }),
    }
}

pub(crate) fn map_execution_state_to_tool_result(
    tool_id: &str,
    state: &ExecutionState,
) -> Result<ToolResult> {
    match state.status {
        ExecutionStatus::Completed => Ok(ToolResult::Success(Value::Null)),
        ExecutionStatus::Suspended => Ok(ToolResult::Suspended(ToolSuspension {
            reason: state
                .suspension_reason
                .clone()
                .unwrap_or_else(|| "User input required".to_string()),
            continuation_state: serde_json::to_value(state)?,
        })),
        ExecutionStatus::Failed => Err(ReadyError::Tool {
            tool_id: tool_id.to_string(),
            message: state
                .error
                .as_ref()
                .map(|error| error.message.clone())
                .unwrap_or_else(|| "Process-backed tool execution failed".to_string()),
        }),
        _ => Err(ReadyError::Tool {
            tool_id: tool_id.to_string(),
            message: format!("Unexpected process execution state: {:?}", state.status),
        }),
    }
}

/// Loads all `*_plan.json` files from a directory into a map keyed by plan name.
pub fn load_plans_from_directory(path: impl AsRef<Path>) -> Result<HashMap<String, AbstractPlan>> {
    let mut plans = HashMap::new();
    let directory = path.as_ref();
    if !directory.exists() {
        return Ok(plans);
    }

    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        if !file_name.ends_with("_plan.json") {
            continue;
        }

        let content = fs::read_to_string(&path)?;
        let plan: AbstractPlan = serde_json::from_str(&content)?;
        plans.insert(plan.name.clone(), plan);
    }

    Ok(plans)
}

fn validate_process_plans(
    plans: &HashMap<String, AbstractPlan>,
    registry: &InMemoryToolRegistry,
) -> Result<()> {
    for plan in plans.values() {
        for tool_id in plan.collect_tool_ids() {
            if plans.contains_key(&tool_id) || registry.get_module_for_tool(&tool_id).is_some() {
                continue;
            }

            return Err(ReadyError::PlanValidation(format!(
                "Unknown tool referenced by process plan '{}': {}",
                plan.name, tool_id
            )));
        }
    }

    Ok(())
}

/// Executes a saved plan as a tool, including suspend-and-resume behavior.
#[async_trait]
impl ToolsModule for ProcessToolsModule {
    fn tools(&self) -> &[ToolDescription] {
        &self.descriptions
    }

    async fn execute(&self, call: &ToolCall) -> Result<ToolResult> {
        let tool_id = call.tool_id.as_str();
        let plan = self
            .plans
            .get(tool_id)
            .ok_or_else(|| ReadyError::ToolNotFound(tool_id.to_string()))?;

        let mut state = if call.continuation.is_some() {
            restore_execution_state(call)?
        } else {
            seed_execution_state(&self.descriptions, tool_id, &call.args)?
        };

        if let Some(cont) = &call.continuation {
            if let Some(resume_value) = cont.resume_value.clone() {
                self.runner
                    .provide_input(self.registry.clone(), plan, &mut state, resume_value)
                    .await?;
            } else {
                self.runner
                    .execute(self.registry.clone(), plan, &mut state)
                    .await?;
            }
        } else {
            self.runner
                .execute(self.registry.clone(), plan, &mut state)
                .await?;
        }

        map_execution_state_to_tool_result(tool_id, &state)
    }
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
