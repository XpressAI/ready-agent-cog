//! Process-backed tools that expose saved abstract plans as callable sub-plan tools.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

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
}

impl ProcessToolsModule {
    /// Constructs a process tools module and validates referenced tools up front.
    pub fn new(
        plans: HashMap<String, AbstractPlan>,
        registry: InMemoryToolRegistry,
    ) -> Result<Self> {
        validate_process_plans(&plans, &registry)?;
        let registry = Arc::new(registry);

        let descriptions = plans
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
            .collect();

        Ok(Self {
            plans,
            descriptions,
            registry,
        })
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
impl ToolsModule for ProcessToolsModule {
    fn tools(&self) -> &[ToolDescription] {
        &self.descriptions
    }

    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let tool_id = call.tool_id.as_str();
            let plan = self
                .plans
                .get(tool_id)
                .ok_or_else(|| ReadyError::ToolNotFound(tool_id.to_string()))?;

            let mut state = if let Some(cont) = &call.continuation {
                serde_json::from_value::<ExecutionState>(cont.state.clone())?
            } else {
                let mut state = ExecutionState::default();
                let description = self
                    .descriptions
                    .iter()
                    .find(|d| d.id == tool_id)
                    .ok_or_else(|| ReadyError::ToolNotFound(tool_id.to_string()))?;

                for (index, argument) in description.arguments.iter().enumerate() {
                    if let Some(value) = call.args.get(index) {
                        state
                            .interpreter_state
                            .variables
                            .insert(argument.name.clone(), value.clone());
                    }
                }
                state
            };

            let interpreter = PlanInterpreter::new(self.registry.clone(), plan.clone());

            if let Some(cont) = &call.continuation {
                if let Some(resume_value) = cont.resume_value.clone() {
                    interpreter.provide_input(&mut state, resume_value).await?;
                } else {
                    interpreter.execute(&mut state).await?;
                }
            } else {
                interpreter.execute(&mut state).await?;
            }

            match state.status {
                ExecutionStatus::Completed => Ok(ToolResult::Success(Value::Null)),
                ExecutionStatus::Suspended => Ok(ToolResult::Suspended(ToolSuspension {
                    reason: state
                        .suspension_reason
                        .clone()
                        .unwrap_or_else(|| "User input required".to_string()),
                    continuation_state: serde_json::to_value(&state)?,
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
        })
    }
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
