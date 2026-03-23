//! Ready is a plan-based agent execution engine for parsing, validating, and running
//! Python-like plans.
//!
//! The crate turns plan source into an abstract syntax tree, validates it against
//! runtime rules and available tools, and executes it by dispatching tool calls
//! through a pluggable tool registry.
//!
//! Key modules include [`plan`] for the AST, [`planning`] for parsing and
//! validation, [`execution`] for interpretation and runtime state, [`tools`]
//! for the tool abstraction layer, [`llm`] for language-model integration,
//! [`workflow`] for higher-level orchestration, and [`error`] for shared error
//! types.

#[cfg(test)]
pub mod test_helpers;

#[cfg(test)]
mod error_tests;

/// Shared error types and result aliases used throughout the crate.
pub mod error;
/// Runtime execution, interpretation, and state-management components.
pub mod execution;
/// Language-model clients and related integration traits.
pub mod llm;
/// The abstract plan data model used across parsing, validation, and execution.
pub mod plan;
/// Formatting helpers for rendering plans and expressions back to text.
pub mod plan_format;
/// Parsing and validation utilities for turning source code into checked plans.
pub mod planning;
/// Tool abstractions, registries, and built-in tool implementations.
pub mod tools;
/// High-level planning and execution workflows built on top of the core engine.
pub mod workflow;

/// Crate-wide error type and standard result alias.
pub use error::{ReadyError, Result};
/// Minimal tool extension surface exposed at the crate root.
pub use tools::{
    ToolArgumentDescription, ToolDescription, ToolResult, ToolReturnDescription, ToolSuspension,
    ToolsModule,
};
