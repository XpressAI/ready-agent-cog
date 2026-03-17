use serde::{Deserialize, Serialize};

/// Indicates whether a plan diagnostic is blocking or informational.
///
/// Validation uses this to separate issues that must stop execution from issues
/// that are useful to surface but do not necessarily invalidate the plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    /// The diagnostic represents a validation failure that should block execution.
    #[serde(rename = "error")]
    Error,
    /// The diagnostic highlights a non-fatal issue worth surfacing to callers.
    #[serde(rename = "warning")]
    Warning,
}

/// A validation diagnostic describing an issue found in a plan.
///
/// Diagnostics are produced during validation so callers can present actionable
/// feedback without relying only on hard failures.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanDiagnostic {
    /// Whether the diagnostic is a blocking error or a warning.
    pub severity: DiagnosticSeverity,
    /// Human-readable text describing the issue and its impact.
    pub message: String,
    /// The related variable name when the issue is tied to a specific symbol.
    pub variable_name: Option<String>,
}
