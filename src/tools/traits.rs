//! Core trait that defines the primary extension point for adding tools.

use async_trait::async_trait;

use crate::error::Result;
use crate::tools::models::{ToolCall, ToolDescription, ToolResult};

/// Describes and executes a set of tools.
///
/// Implementors store their [`ToolDescription`] list and return a borrowed slice
/// from [`tools`](ToolsModule::tools). Execution is dispatched via
/// [`execute`](ToolsModule::execute) using a structured [`ToolCall`].
#[async_trait]
pub trait ToolsModule: Send + Sync {
    /// Returns the descriptions of all tools provided by this module.
    fn tools(&self) -> &[ToolDescription];

    /// Executes the tool identified by `call.tool_id` with the given arguments
    /// and optional suspension continuation.
    async fn execute(&self, call: &ToolCall) -> Result<ToolResult>;
}
