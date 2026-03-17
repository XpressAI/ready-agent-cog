use std::collections::HashMap;
use std::pin::Pin;

use ready::error::Result;
use ready::plan::{AbstractPlan, Step};
use ready::tools::models::{ToolCall, ToolDescription, ToolResult, ToolReturnDescription};
use ready::tools::{InMemoryToolRegistry, ProcessToolsModule, ToolsModule};
use serde_json::Value;

struct MockModule {
    descriptions: Vec<ToolDescription>,
}

impl ToolsModule for MockModule {
    fn tools(&self) -> &[ToolDescription] {
        &self.descriptions
    }

    fn execute<'a>(
        &'a self,
        _call: &'a ToolCall,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send + 'a>> {
        Box::pin(async { Ok(ToolResult::Success(Value::Null)) })
    }
}

fn noop_tool_description() -> ToolDescription {
    ToolDescription {
        id: "noop".to_string(),
        description: String::new(),
        arguments: vec![],
        returns: ToolReturnDescription {
            name: None,
            description: String::new(),
            type_name: None,
            fields: vec![],
        },
    }
}

fn child_plan() -> AbstractPlan {
    AbstractPlan {
        name: "child_plan".to_string(),
        description: "Reusable sub-plan".to_string(),
        steps: vec![Step::ToolStep {
            tool_id: "noop".to_string(),
            arguments: vec![],
            output_variable: None,
        }],
        code: String::new(),
    }
}

#[test]
fn process_tools_module_registers_tools_in_registry() {
    let mut registry = InMemoryToolRegistry::new();
    registry
        .register_module(Box::new(MockModule {
            descriptions: vec![noop_tool_description()],
        }))
        .expect("mock module should register");

    let process_module = ProcessToolsModule::new(
        HashMap::from([("child_plan".to_string(), child_plan())]),
        registry.clone(),
    )
    .expect("process module should construct");

    assert_eq!(process_module.tools().len(), 1);
    assert_eq!(process_module.tools()[0].id, "child_plan");

    registry
        .register_module(Box::new(process_module))
        .expect("process module should register");

    assert!(registry.has_tool("child_plan"));
    assert!(registry.get_module_for_tool("child_plan").is_some());
}
