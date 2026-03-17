use super::*;

fn field(name: &str, type_name: &str) -> FieldDescription {
    FieldDescription {
        name: name.to_string(),
        description: String::new(),
        type_name: type_name.to_string(),
        fields: Vec::new(),
    }
}

fn described_field(name: &str, type_name: &str, description: &str) -> FieldDescription {
    FieldDescription {
        name: name.to_string(),
        description: description.to_string(),
        type_name: type_name.to_string(),
        fields: Vec::new(),
    }
}

fn make_tool(
    tool_id: &str,
    return_type: Option<&str>,
    fields: Vec<FieldDescription>,
) -> ToolDescription {
    ToolDescription {
        id: tool_id.to_string(),
        description: String::new(),
        arguments: Vec::new(),
        returns: ToolReturnDescription {
            name: None,
            description: String::new(),
            type_name: return_type.map(str::to_string),
            fields,
        },
    }
}

#[test]
fn render_class_stub_basic() {
    let stub = render_class_stub("MyType", &[field("id", "str"), field("score", "float")]);

    assert_eq!(stub, "class MyType:\n    id: str\n    score: float");
}

#[test]
fn render_class_stub_with_descriptions() {
    let stub = render_class_stub(
        "Item",
        &[
            described_field("name", "str", "The name"),
            described_field("count", "int", "How many"),
        ],
    );

    assert!(stub.contains("# The name"));
    assert!(stub.contains("# How many"));
    assert!(stub.contains("name: str  # The name"));
    assert!(stub.contains("count: int  # How many"));
}

#[test]
fn generate_prompt_stubs_deduplicates_class() {
    let fields = vec![field("x", "int")];
    let tool1 = make_tool("tool1", Some("Shared"), fields.clone());
    let tool2 = make_tool("tool2", Some("Shared"), fields);

    let result = generate_prompt_stubs(&[tool1, tool2]);

    assert_eq!(result.matches("class Shared:").count(), 1);
}

#[test]
fn generate_prompt_stubs_no_fields_no_class() {
    let tool = ToolDescription {
        id: "get_name".to_string(),
        description: String::new(),
        arguments: Vec::new(),
        returns: ToolReturnDescription {
            name: None,
            description: String::new(),
            type_name: Some("str".to_string()),
            fields: Vec::new(),
        },
    };

    let result = generate_prompt_stubs(&[tool]);

    assert!(!result.contains("class "));
    assert!(result.contains("def get_name"));
}

#[test]
fn generate_prompt_stubs_classes_before_functions() {
    let tool = make_tool("get_item", Some("Item"), vec![field("val", "int")]);

    let result = generate_prompt_stubs(&[tool]);

    let class_pos = result.find("class Item:").expect("class stub should exist");
    let func_pos = result
        .find("def get_item")
        .expect("function stub should exist");
    assert!(class_pos < func_pos);
}

#[test]
fn generate_prompt_stubs_nested_types_in_dependency_order() {
    let tool = make_tool(
        "get_outer",
        Some("Outer"),
        vec![FieldDescription {
            name: "child".to_string(),
            description: String::new(),
            type_name: "Inner".to_string(),
            fields: vec![field("v", "int")],
        }],
    );

    let result = generate_prompt_stubs(&[tool]);

    let inner_pos = result
        .find("class Inner:")
        .expect("inner class should exist");
    let outer_pos = result
        .find("class Outer:")
        .expect("outer class should exist");
    assert!(inner_pos < outer_pos);
}

#[test]
fn to_python_stub_unchanged_for_simple_tools() {
    let tool = ToolDescription {
        id: "summarize".to_string(),
        description: "Summarize a message.".to_string(),
        arguments: Vec::new(),
        returns: ToolReturnDescription {
            name: None,
            description: String::new(),
            type_name: Some("str".to_string()),
            fields: Vec::new(),
        },
    };

    assert_eq!(
        tool.to_python_stub(),
        "def summarize() -> str:\n    \"\"\"Summarize a message.\"\"\"\n    ..."
    );
}
