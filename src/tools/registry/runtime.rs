use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::{ReadyError, Result};
use crate::tools::models::{ToolCall, ToolDescription, ToolResult};
use crate::tools::traits::ToolsModule;

#[derive(Clone)]
struct RegisteredModule {
    module: Arc<dyn ToolsModule>,
}

/// In-memory registry that stores tool modules. Uses modules as the single source of truth,
/// deriving flat views on-the-fly (not a hot path — called during setup, not execution).
pub struct InMemoryToolRegistry {
    modules: Vec<RegisteredModule>,
    module_by_tool_id: HashMap<String, usize>,
}

impl Clone for InMemoryToolRegistry {
    fn clone(&self) -> Self {
        Self {
            modules: self.modules.clone(),
            module_by_tool_id: self.module_by_tool_id.clone(),
        }
    }
}

impl InMemoryToolRegistry {
    /// Creates an empty registry with no registered tool modules.
    pub fn new() -> Self {
        Self {
            modules: Vec::new(),
            module_by_tool_id: HashMap::new(),
        }
    }

    /// Registers a tool module and updates all owned lookup state immediately.
    /// Returns an error if any tool ID is already registered.
    pub fn register_module(&mut self, module: Box<dyn ToolsModule>) -> Result<()> {
        let module: Arc<dyn ToolsModule> = Arc::from(module);
        let module_index = self.modules.len();

        for tool in module.tools() {
            let tool_id = tool.id.clone();

            if self.module_by_tool_id.contains_key(&tool_id) {
                return Err(ReadyError::DuplicateTool(tool_id));
            }

            self.module_by_tool_id.insert(tool_id, module_index);
        }

        self.modules.push(RegisteredModule { module });

        Ok(())
    }

    /// Returns the module that owns the given tool ID, if one is registered.
    pub fn get_module_for_tool(&self, tool_id: &str) -> Option<&dyn ToolsModule> {
        let module_index = self.module_by_tool_id.get(tool_id).copied()?;
        Some(self.modules[module_index].module.as_ref())
    }

    pub fn has_tool(&self, tool_id: &str) -> bool {
        self.module_by_tool_id.contains_key(tool_id)
    }

    /// Returns a flat list of all tool descriptions from all registered modules.
    /// Deduplicates by tool ID, keeping the description from the owning module (not a hot path).
    pub fn tools(&self) -> Vec<ToolDescription> {
        // Build a map of tool_id -> description from the owning module
        let mut tool_map: HashMap<String, ToolDescription> = HashMap::new();

        for module in &self.modules {
            for desc in module.module.tools() {
                tool_map.insert(desc.id.clone(), desc.clone());
            }
        }

        tool_map.into_values().collect()
    }

    /// Executes a tool by routing to the owning module.
    pub fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            let module = self
                .get_module_for_tool(&call.tool_id)
                .ok_or_else(|| ReadyError::ToolNotFound(call.tool_id.clone()))?;
            module.execute(call).await
        })
    }
}

impl Default for InMemoryToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
