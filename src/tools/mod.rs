//! Tool abstractions, concrete implementations, and registry types for the Ready execution engine.

/// Built-in tool implementations backed by shared runtime services.
pub mod builtin;
/// Shared data models used to describe tools, arguments, and execution results.
pub mod models;
/// Process-backed tools that expose saved plans as callable tools.
pub mod process;
/// Registry types for resolving tool IDs to their owning modules.
pub mod registry;
/// Shell-backed tools that execute templated commands and parse their output.
pub mod shell;
/// Core extension traits implemented by tool modules.
pub mod traits;

/// Re-exports the built-in tools module implementation.
pub use builtin::BuiltinToolsModule;
/// Re-exports the core tool description and execution result models.
pub use models::{
    FieldDescription, ToolArgumentDescription, ToolDescription, ToolResult, ToolReturnDescription,
    ToolSuspension, generate_prompt_stubs, render_class_stub,
};
/// Re-exports the process-backed tools module implementation.
pub use process::ProcessToolsModule;
/// Re-exports the in-memory tool registry.
pub use registry::InMemoryToolRegistry;
/// Re-exports shell tool definitions, storage helpers, and runtime module types.
pub use shell::{OutputParsing, ShellToolEntry, ShellToolStore, ShellToolsModule};
/// Re-exports the core trait required to execute tools.
pub use traits::ToolsModule;
