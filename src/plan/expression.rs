use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};

/// A typed literal value used inside the core AST.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum LiteralValue {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Array(Vec<LiteralValue>),
    Object(BTreeMap<String, LiteralValue>),
}

impl LiteralValue {
    pub fn to_json_value(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Bool(value) => Value::Bool(*value),
            Self::Integer(value) => Value::Number(Number::from(*value)),
            Self::Float(value) => Number::from_f64(*value)
                .map(Value::Number)
                .unwrap_or(Value::Null),
            Self::String(value) => Value::String(value.clone()),
            Self::Array(values) => {
                Value::Array(values.iter().map(LiteralValue::to_json_value).collect())
            }
            Self::Object(values) => Value::Object(
                values
                    .iter()
                    .map(|(key, value)| (key.clone(), value.to_json_value()))
                    .collect(),
            ),
        }
    }
}

impl std::fmt::Display for LiteralValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match serde_json::to_string(&self.to_json_value()) {
            Ok(text) => f.write_str(&text),
            Err(_) => f.write_str("null"),
        }
    }
}

/// Binary operators for arithmetic expressions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BinaryOperator {
    #[serde(rename = "+")]
    Add,
    #[serde(rename = "-")]
    Subtract,
    #[serde(rename = "*")]
    Multiply,
    #[serde(rename = "/")]
    Divide,
    #[serde(rename = "//")]
    FloorDivide,
    #[serde(rename = "%")]
    Modulo,
    #[serde(rename = "**")]
    Power,
}

impl BinaryOperator {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Subtract => "-",
            Self::Multiply => "*",
            Self::Divide => "/",
            Self::FloorDivide => "//",
            Self::Modulo => "%",
            Self::Power => "**",
        }
    }
}

/// Unary operators for prefix numeric expressions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UnaryOperator {
    #[serde(rename = "+")]
    Plus,
    #[serde(rename = "-")]
    Minus,
}

impl UnaryOperator {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Plus => "+",
            Self::Minus => "-",
        }
    }
}

/// Comparison operators for conditional expressions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ComparisonOperator {
    #[serde(rename = "==")]
    Equal,
    #[serde(rename = "!=")]
    NotEqual,
    #[serde(rename = "<")]
    LessThan,
    #[serde(rename = ">")]
    GreaterThan,
    #[serde(rename = "<=")]
    LessThanOrEqual,
    #[serde(rename = ">=")]
    GreaterThanOrEqual,
    #[serde(rename = "in")]
    In,
    #[serde(rename = "not in")]
    NotIn,
    #[serde(rename = "is")]
    Is,
    #[serde(rename = "is not")]
    IsNot,
}

impl ComparisonOperator {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Equal => "==",
            Self::NotEqual => "!=",
            Self::LessThan => "<",
            Self::GreaterThan => ">",
            Self::LessThanOrEqual => "<=",
            Self::GreaterThanOrEqual => ">=",
            Self::In => "in",
            Self::NotIn => "not in",
            Self::Is => "is",
            Self::IsNot => "is not",
        }
    }
}

/// Boolean operators combining multiple operands.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BooleanOperator {
    #[serde(rename = "and")]
    And,
    #[serde(rename = "or")]
    Or,
}

impl BooleanOperator {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Or => "or",
        }
    }
}

/// A single segment in an access path applied after a base variable name.
///
/// Each segment is represented as one closed variant so invalid combinations
/// are impossible to construct in the AST.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "key")]
pub enum Accessor {
    /// Accesses a named field using dot notation such as `.name`.
    #[serde(rename = "attribute")]
    Attribute(String),
    /// Accesses a named key using bracket notation such as `["name"]`.
    #[serde(rename = "key")]
    Key(String),
    /// Accesses a positional index using bracket notation such as `[0]`.
    #[serde(rename = "index")]
    Index(i64),
}

/// An expression node in the abstract plan syntax tree.
///
/// Expressions describe how values are read, combined, and tested inside plan
/// steps before the runtime evaluates them.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Expression {
    /// Reads a variable and optionally traverses nested fields or indices from it.
    AccessPath {
        variable_name: String,
        #[serde(default)]
        accessors: Vec<Accessor>,
    },
    /// Embeds a typed literal value directly in the plan.
    Literal { value: LiteralValue },
    /// Represents an infix arithmetic or other binary operation.
    BinaryExpression {
        operator: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    /// Represents a prefix unary operation applied to a single operand.
    UnaryExpression {
        operator: UnaryOperator,
        operand: Box<Expression>,
    },
    /// Concatenates multiple expression parts into a single combined value.
    ConcatExpression { parts: Vec<Expression> },
    /// Compares two expressions using an operator such as `==` or `>`.
    Comparison {
        operator: ComparisonOperator,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    /// Combines multiple expressions with a boolean operator such as `and` or `or`.
    Boolean {
        operator: BooleanOperator,
        operands: Vec<Expression>,
    },
    /// Negates the truthiness of a nested expression.
    Not { operand: Box<Expression> },
    /// Constructs an object from string keys mapped to arbitrary expressions.
    DictExpression {
        entries: Vec<(String, Expression)>,
    },
    /// Constructs an array from arbitrary expressions.
    ArrayExpression { elements: Vec<Expression> },
}
