//! Shared test utilities for plan construction and expression building.

use crate::plan::{AbstractPlan, Expression, LiteralValue, Step};
use crate::tools::models::{ToolDescription, ToolReturnDescription};

pub fn to_literal(value: serde_json::Value) -> LiteralValue {
    match value {
        serde_json::Value::Null => LiteralValue::Null,
        serde_json::Value::Bool(v) => LiteralValue::Bool(v),
        serde_json::Value::Number(n) => {
            if let Some(v) = n.as_i64() {
                LiteralValue::Integer(v)
            } else {
                LiteralValue::Float(n.as_f64().expect("finite json number"))
            }
        }
        serde_json::Value::String(v) => LiteralValue::String(v),
        serde_json::Value::Array(values) => {
            LiteralValue::Array(values.into_iter().map(to_literal).collect())
        }
        serde_json::Value::Object(values) => LiteralValue::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, to_literal(value)))
                .collect(),
        ),
    }
}

pub fn lit(value: impl serde::Serialize) -> Expression {
    Expression::Literal {
        value: to_literal(serde_json::to_value(value).expect("literal should serialize")),
    }
}

pub fn var(name: &str) -> Expression {
    Expression::AccessPath {
        variable_name: name.to_string(),
        accessors: vec![],
    }
}

pub fn plan(steps: Vec<Step>) -> AbstractPlan {
    AbstractPlan {
        name: "test".to_string(),
        description: String::new(),
        code: String::new(),
        steps,
    }
}

pub fn tool(tool_id: &str) -> ToolDescription {
    ToolDescription {
        id: tool_id.to_string(),
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

pub fn assign(name: &str, value: Expression) -> Step {
    Step::AssignStep {
        target: name.to_string(),
        value,
    }
}
