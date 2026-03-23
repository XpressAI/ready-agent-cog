//! Central error types for the crate.
//!
//! These errors provide a common vocabulary for failures that can happen while
//! parsing plans, validating them, invoking tools, talking to LLMs, or running
//! the execution engine.

use thiserror::Error;

/// The top-level error type used across Ready's parsing, validation, and runtime layers.
///
/// It centralizes the main failure modes exposed by the crate so callers can handle
/// planning, tool, network, and execution problems through a single error surface.
#[derive(Error, Debug)]
pub enum ReadyError {
    /// A Python-like plan could not be parsed into the internal AST representation.
    #[error("Plan parsing error: {0}")]
    PlanParsing(String),
    /// A parsed plan failed semantic validation before execution.
    #[error("Plan validation error: {0}")]
    PlanValidation(String),
    /// An expression could not be evaluated during execution.
    #[error("Expression evaluation error: {0}")]
    Evaluation(String),
    /// A tool reported an execution failure along with its identifier.
    #[error("Tool error: {tool_id}: {message}")]
    Tool { tool_id: String, message: String },
    /// A referenced tool identifier was not found in the active tool registry.
    #[error("Tool not found: {0}")]
    ToolNotFound(String),
    /// A tool identifier was registered more than once in the active registry.
    #[error("Duplicate tool registration: {0}")]
    DuplicateTool(String),
    /// An LLM client or model interaction failed.
    #[error("LLM error: {0}")]
    Llm(String),
    /// The interpreter failed while executing a specific step, if known.
    ///
    /// The optional step metadata helps surface where execution stopped without
    /// requiring a separate traceback structure.
    #[error("Execution error at step {step_index:?}: {message}")]
    Execution {
        step_index: Option<usize>,
        step_type: Option<String>,
        message: String,
    },
    /// An underlying I/O operation failed.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON serialization or deserialization failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// An outbound HTTP request or response handling step failed.
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

impl ReadyError {
    /// Determines if an error can be recovered from via LLM planning.
    ///
    /// Recoverable errors are runtime execution errors where the plan structure
    /// is valid but execution failed (e.g., tool returned unexpected result).
    ///
    /// Unrecoverable errors include:
    /// - ToolNotFound: The tool doesn't exist, can't recover
    /// - PlanParsing: The plan is invalid, can't recover
    /// - PlanValidation: The plan failed validation, can't recover
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            ReadyError::Execution { .. } | ReadyError::Tool { .. }
        )
    }
}

/// A crate-wide convenience alias for results that return [`ReadyError`].
///
/// This keeps public APIs concise while ensuring failures consistently use the
/// crate's shared error type.
pub type Result<T> = std::result::Result<T, ReadyError>;
