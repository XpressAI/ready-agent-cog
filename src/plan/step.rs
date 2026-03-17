use serde::{Deserialize, Serialize};

use super::Expression;

/// Semantic branch identity for switch-style conditionals.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum BranchKind {
    #[serde(rename = "if")]
    If,
    #[serde(rename = "elif")]
    ElseIf,
    #[serde(rename = "else")]
    Else,
}

/// A single branch in a switch-style conditional structure.
///
/// Branches preserve semantic branch role and nested body so the formatter and
/// interpreter can retain if/elif/else behavior without storing presentation labels.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConditionalBranch {
    /// The semantic kind of branch represented in the AST.
    pub kind: BranchKind,
    /// The condition that must hold for the branch to execute, if any.
    pub condition: Option<Expression>,
    #[serde(default)]
    /// The steps executed when this branch is selected.
    pub steps: Vec<Step>,
}

/// A single executable statement in a plan.
///
/// Steps form the runtime control-flow tree that the interpreter walks when it
/// executes a validated plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum Step {
    /// Assigns the result of an expression to a named variable.
    AssignStep { target: String, value: Expression },
    /// Invokes a registered tool and optionally stores its result in a variable.
    ToolStep {
        tool_id: String,
        #[serde(default)]
        arguments: Vec<Expression>,
        output_variable: Option<String>,
    },
    /// Evaluates ordered conditional branches and executes the first matching branch.
    SwitchStep {
        #[serde(default)]
        branches: Vec<ConditionalBranch>,
    },
    /// Iterates over an iterable variable and runs the body once per item.
    LoopStep {
        iterable_variable: String,
        item_variable: String,
        #[serde(default)]
        body: Vec<Step>,
    },
    /// Repeats the body while the condition continues to evaluate as truthy.
    WhileStep {
        condition: Expression,
        #[serde(default)]
        body: Vec<Step>,
    },
    /// Requests input from a user, optionally binding the response to a variable.
    UserInteractionStep {
        prompt: String,
        output_variable: Option<String>,
    },
}
