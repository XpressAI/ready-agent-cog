//! Execution state types tracking the interpreter's runtime position, variables, and results.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::execution::observer::step_type_name;
use crate::plan::Step;

/// High-level execution status for a plan run.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutionStatus {
    /// The execution has not started yet.
    #[serde(rename = "pending")]
    #[default]
    Pending,
    /// The execution is currently running.
    #[serde(rename = "running")]
    Running,
    /// The execution is paused and waiting for external input.
    #[serde(rename = "suspended")]
    Suspended,
    /// The execution finished successfully.
    #[serde(rename = "completed")]
    Completed,
    /// The execution terminated with an error.
    #[serde(rename = "failed")]
    Failed,
}

/// Tracks the current iteration index and source items for a `for`-style loop.
#[derive(Debug, Clone, PartialEq)]
pub struct LoopState {
    pub index: usize,
    pub items: Vec<Value>,
}

/// Tracks how many iterations a `while` loop has executed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhileState {
    pub iterations: usize,
}

/// Stack of indices describing the current position within the plan tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionPointer {
    pub path: Vec<usize>,
}

impl InstructionPointer {
    /// Creates an instruction pointer positioned at the first top-level step.
    pub fn new() -> Self {
        Self { path: vec![0] }
    }

    /// Advances to the next step at the current depth.
    pub fn advance(&mut self) {
        if let Some(last) = self.path.last_mut() {
            *last += 1;
        }
    }

    /// Descends into the first child step of the current container.
    pub fn descend(&mut self) {
        self.path.push(0);
    }

    /// Ascends to the parent container and advances past the completed child scope.
    pub fn ascend(&mut self) {
        if self.path.len() <= 1 {
            panic!("Cannot ascend beyond the root level");
        }
        self.path.pop();
        if let Some(last) = self.path.last_mut() {
            *last += 1;
        }
    }

    /// Returns the current path depth.
    pub fn depth(&self) -> usize {
        self.path.len()
    }

    /// Returns a cloned snapshot of the current instruction path.
    pub fn snapshot(&self) -> Vec<usize> {
        self.path.clone()
    }
}

impl Default for InstructionPointer {
    fn default() -> Self {
        Self::new()
    }
}

impl TryFrom<Vec<usize>> for InstructionPointer {
    type Error = &'static str;

    fn try_from(path: Vec<usize>) -> Result<Self, Self::Error> {
        if path.is_empty() {
            return Err("Instruction pointer path cannot be empty");
        }

        Ok(Self { path })
    }
}

/// Internal bookkeeping for loop counters, while counters, and selected switch branches.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct InternalState {
    pub loops: HashMap<Vec<usize>, LoopState>,
    pub whiles: HashMap<Vec<usize>, WhileState>,
    pub branches: HashMap<Vec<usize>, usize>,
}

/// Serializable interpreter runtime state, including variables and pending continuations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterpreterState {
    #[serde(default = "default_ip_path")]
    pub ip_path: Vec<usize>,
    #[serde(default)]
    pub variables: HashMap<String, Value>,
    #[serde(default)]
    pub pending_input_variable: Option<String>,
    #[serde(default)]
    pub pending_tool_id: Option<String>,
    #[serde(default)]
    pub pending_tool_state: Option<Value>,
    #[serde(default)]
    pub pending_resume_value: Option<Value>,
}

fn default_ip_path() -> Vec<usize> {
    vec![0]
}

/// Structured execution error annotated with step location and message details.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionError {
    #[serde(default)]
    pub step_index: Option<usize>,
    #[serde(default)]
    pub step_type: Option<String>,
    pub exception_type: String,
    pub message: String,
}

/// Records step completion metadata, including whether execution suspended.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StepResult {
    #[serde(default)]
    pub suspend: bool,
    pub suspend_reason: Option<String>,
}

/// Top-level execution state containing status, interpreter state, and terminal details.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExecutionState {
    #[serde(default)]
    pub status: ExecutionStatus,
    #[serde(default)]
    pub interpreter_state: InterpreterState,
    #[serde(default)]
    pub current_step_index: Option<usize>,
    #[serde(default)]
    pub error: Option<ExecutionError>,
    #[serde(default)]
    pub suspension_reason: Option<String>,
}

impl Default for ExecutionState {
    fn default() -> Self {
        Self {
            status: ExecutionStatus::Pending,
            interpreter_state: InterpreterState {
                ip_path: default_ip_path(),
                variables: HashMap::new(),
                pending_input_variable: None,
                pending_tool_id: None,
                pending_tool_state: None,
                pending_resume_value: None,
            },
            current_step_index: None,
            error: None,
            suspension_reason: None,
        }
    }
}

impl Default for InterpreterState {
    fn default() -> Self {
        Self {
            ip_path: default_ip_path(),
            variables: HashMap::new(),
            pending_input_variable: None,
            pending_tool_id: None,
            pending_tool_state: None,
            pending_resume_value: None,
        }
    }
}

impl ExecutionError {
    /// Constructs an execution error from step context and message details.
    pub fn from_step(
        step_index: Option<usize>,
        step: Option<&Step>,
        exception_type: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            step_index,
            step_type: step.map(|s| step_type_name(s).to_string()),
            exception_type: exception_type.into(),
            message: message.into(),
        }
    }
}
