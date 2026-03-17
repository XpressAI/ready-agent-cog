//! The abstract plan data model used by Ready.
//!
//! This module defines the AST-like representation that the planner produces,
//! the validator checks, and the execution engine interprets at runtime.

mod diagnostics;
mod expression;
mod queries;
mod step;
mod types;

#[cfg(test)]
mod tests;

pub use diagnostics::{DiagnosticSeverity, PlanDiagnostic};
pub use expression::{
    Accessor, BinaryOperator, BooleanOperator, ComparisonOperator, Expression, LiteralValue,
    UnaryOperator,
};
pub use step::{BranchKind, ConditionalBranch, Step};
pub use types::{AbstractPlan, PrefillableInput};
