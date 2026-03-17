use serde::{Deserialize, Serialize};

use super::Step;

/// A top-level user input request that can be pre-supplied before execution starts.
///
/// This gives workflow code a stable way to discover required human inputs before
/// the interpreter begins executing the plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PrefillableInput {
    /// The variable name that will receive the collected user input.
    pub variable_name: String,
    /// The prompt shown to the user when the value is not prefilled.
    pub prompt: String,
}

/// The top-level representation of a parsed plan.
///
/// It bundles plan metadata, executable steps, and source text so the same value
/// can support validation, execution, formatting, and debugging workflows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AbstractPlan {
    /// A human-readable name that identifies the plan.
    pub name: String,
    #[serde(default)]
    /// A short description explaining the plan's purpose.
    pub description: String,
    #[serde(default)]
    /// The executable steps that make up the plan body.
    pub steps: Vec<Step>,
    #[serde(default)]
    /// The original or reconstructed source code associated with the plan.
    pub code: String,
}
