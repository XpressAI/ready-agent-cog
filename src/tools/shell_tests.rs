use super::*;
use crate::error::ReadyError;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

fn entry(template: &[&str]) -> ShellToolEntry {
    ShellToolEntry {
        description: String::new(),
        template: template.iter().map(|part| (*part).to_string()).collect(),
        arguments: Vec::new(),
        returns: ToolReturnDescription {
            name: None,
            description: String::new(),
            type_name: Some("str".to_string()),
            fields: Vec::new(),
        },
        active: true,
        output_parsing: OutputParsing::Raw,
        output_schema: None,
    }
}

fn argument(
    name: &str,
    type_name: &str,
    description: &str,
    default: Option<&str>,
) -> ToolArgumentDescription {
    ToolArgumentDescription {
        name: name.to_string(),
        description: description.to_string(),
        type_name: type_name.to_string(),
        default: default.map(str::to_string),
    }
}

fn shell_module(entries: HashMap<String, ShellToolEntry>) -> ShellToolsModule {
    ShellToolsModule::new(entries)
}

fn unique_temp_file(name: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("ready_{name}_{nanos}.json"))
}

#[test]
fn shell_store_load_returns_empty_when_missing() {
    let path = unique_temp_file("missing_shell_store");
    let loaded = ShellToolStore::load(&path).expect("missing file should yield empty store");
    assert!(loaded.is_empty());
}

#[test]
fn shell_store_save_and_load_round_trip_entries() {
    let path = unique_temp_file("shell_store_roundtrip");
    let mut entries = HashMap::new();
    entries.insert(
        "greet".to_string(),
        ShellToolEntry {
            description: "Say hello".to_string(),
            template: vec!["echo".to_string(), "{message}".to_string()],
            arguments: vec![argument("message", "str", "Greeting", None)],
            returns: ToolReturnDescription {
                name: Some("output".to_string()),
                description: "Output text".to_string(),
                type_name: Some("str".to_string()),
                fields: Vec::new(),
            },
            active: true,
            output_parsing: OutputParsing::Raw,
            output_schema: None,
        },
    );

    ShellToolStore::save(&path, &entries).expect("save should succeed");
    let restored = ShellToolStore::load(&path).expect("load should succeed");
    assert_eq!(restored, entries);

    let _ = fs::remove_file(path);
}

#[test]
fn list_tools_returns_only_active_entries_with_preserved_fields() {
    let mut active = entry(&["cmd", "/c", "echo", "hello"]);
    active.description = "Echo a line".to_string();
    active.arguments = vec![argument("message", "str", "Message to echo", None)];
    active.returns = ToolReturnDescription {
        name: Some("output".to_string()),
        description: "Captured output".to_string(),
        type_name: Some("str".to_string()),
        fields: Vec::new(),
    };

    let mut inactive = entry(&["cmd", "/c", "echo", "nope"]);
    inactive.active = false;

    let module = shell_module(HashMap::from([
        ("active_tool".to_string(), active.clone()),
        ("inactive_tool".to_string(), inactive),
    ]));

    let listed = module.tools();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "active_tool");
    assert_eq!(listed[0].description, active.description);
    assert_eq!(listed[0].arguments, active.arguments);
    assert_eq!(listed[0].returns, active.returns);
}

#[tokio::test]
async fn execute_tool_binds_arguments_and_runs_command() {
    let module = shell_module(HashMap::from([(
        "echo_message".to_string(),
        ShellToolEntry {
            description: "Echo a message".to_string(),
            template: vec![
                "cmd".to_string(),
                "/c".to_string(),
                "echo".to_string(),
                "{message}".to_string(),
            ],
            arguments: vec![argument("message", "str", "Message", None)],
            returns: ToolReturnDescription {
                name: Some("output".to_string()),
                description: String::new(),
                type_name: Some("str".to_string()),
                fields: Vec::new(),
            },
            active: true,
            output_parsing: OutputParsing::Raw,
            output_schema: None,
        },
    )]));

    use crate::tools::models::ToolCall;
    let call = ToolCall {
        tool_id: "echo_message".to_string(),
        args: vec![json!("hello")],
        continuation: None,
    };
    let result = module.execute(&call).await.expect("command should execute");

    match result {
        ToolResult::Success(Value::String(output)) => {
            assert!(output.to_ascii_lowercase().contains("hello"))
        }
        other => panic!("expected string success result, got {other:?}"),
    }
}

#[tokio::test]
async fn execute_tool_reports_missing_placeholder_contextually() {
    let module = shell_module(HashMap::from([(
        "grep_search".to_string(),
        ShellToolEntry {
            description: String::new(),
            template: vec![
                "grep".to_string(),
                "{pattern}".to_string(),
                "{path}".to_string(),
            ],
            arguments: vec![argument("pattern", "str", "", None)],
            returns: ToolReturnDescription {
                name: None,
                description: String::new(),
                type_name: Some("str".to_string()),
                fields: Vec::new(),
            },
            active: true,
            output_parsing: OutputParsing::Raw,
            output_schema: None,
        },
    )]));

    use crate::tools::models::ToolCall;
    let call = ToolCall {
        tool_id: "grep_search".to_string(),
        args: vec![json!("foo")],
        continuation: None,
    };
    let error = module
        .execute(&call)
        .await
        .expect_err("missing placeholder should error");

    match error {
        ReadyError::Tool { tool_id, message } => {
            assert_eq!(tool_id, "grep_search");
            assert!(message.contains("Template placeholder 'path' not satisfied"));
        }
        other => panic!("expected tool error, got {other:?}"),
    }
}

#[test]
fn output_parsing_serializes_with_expected_json_names() {
    assert_eq!(
        serde_json::to_string(&OutputParsing::Raw).expect("raw should serialize"),
        r#""raw""#
    );
    assert_eq!(
        serde_json::to_string(&OutputParsing::Json).expect("json should serialize"),
        r#""json""#
    );
    assert_eq!(
        serde_json::to_string(&OutputParsing::Int).expect("int should serialize"),
        r#""int""#
    );
    assert_eq!(
        serde_json::to_string(&OutputParsing::Float).expect("float should serialize"),
        r#""float""#
    );
    assert_eq!(
        serde_json::to_string(&OutputParsing::Bool).expect("bool should serialize"),
        r#""bool""#
    );
}

#[test]
fn output_parsing_deserializes_supported_values() {
    assert_eq!(
        serde_json::from_str::<OutputParsing>(r#""raw""#).expect("raw should parse"),
        OutputParsing::Raw
    );
    assert_eq!(
        serde_json::from_str::<OutputParsing>(r#""json""#).expect("json should parse"),
        OutputParsing::Json
    );
    assert_eq!(
        serde_json::from_str::<OutputParsing>(r#""int""#).expect("int should parse"),
        OutputParsing::Int
    );
    assert_eq!(
        serde_json::from_str::<OutputParsing>(r#""float""#).expect("float should parse"),
        OutputParsing::Float
    );
    assert_eq!(
        serde_json::from_str::<OutputParsing>(r#""bool""#).expect("bool should parse"),
        OutputParsing::Bool
    );
}

#[tokio::test]
async fn execute_tool_parses_int_float_and_bool() {
    let module = shell_module(HashMap::from([
        (
            "int_tool".to_string(),
            ShellToolEntry {
                description: String::new(),
                template: vec![
                    "cmd".to_string(),
                    "/c".to_string(),
                    "echo".to_string(),
                    "42".to_string(),
                ],
                arguments: Vec::new(),
                returns: ToolReturnDescription {
                    name: None,
                    description: String::new(),
                    type_name: Some("int".to_string()),
                    fields: Vec::new(),
                },
                active: true,
                output_parsing: OutputParsing::Int,
                output_schema: None,
            },
        ),
        (
            "float_tool".to_string(),
            ShellToolEntry {
                description: String::new(),
                template: vec![
                    "cmd".to_string(),
                    "/c".to_string(),
                    "echo".to_string(),
                    "3.14".to_string(),
                ],
                arguments: Vec::new(),
                returns: ToolReturnDescription {
                    name: None,
                    description: String::new(),
                    type_name: Some("float".to_string()),
                    fields: Vec::new(),
                },
                active: true,
                output_parsing: OutputParsing::Float,
                output_schema: None,
            },
        ),
        (
            "bool_tool".to_string(),
            ShellToolEntry {
                description: String::new(),
                template: vec![
                    "cmd".to_string(),
                    "/c".to_string(),
                    "echo".to_string(),
                    "yes".to_string(),
                ],
                arguments: Vec::new(),
                returns: ToolReturnDescription {
                    name: None,
                    description: String::new(),
                    type_name: Some("bool".to_string()),
                    fields: Vec::new(),
                },
                active: true,
                output_parsing: OutputParsing::Bool,
                output_schema: None,
            },
        ),
    ]));

    use crate::tools::models::ToolCall;
    let mk = |id: &str| ToolCall {
        tool_id: id.to_string(),
        args: vec![],
        continuation: None,
    };
    assert_eq!(
        module
            .execute(&mk("int_tool"))
            .await
            .expect("int parse should work"),
        ToolResult::Success(json!(42))
    );
    assert_eq!(
        module
            .execute(&mk("float_tool"))
            .await
            .expect("float parse should work"),
        ToolResult::Success(json!(3.14))
    );
    assert_eq!(
        module
            .execute(&mk("bool_tool"))
            .await
            .expect("bool parse should work"),
        ToolResult::Success(json!(true))
    );
}
