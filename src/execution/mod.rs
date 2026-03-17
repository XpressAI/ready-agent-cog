//! Plan execution engine for interpreting `AbstractPlan` ASTs by walking steps,
//! evaluating expressions, and dispatching tool calls.

/// Expression evaluation helpers for runtime variable scopes.
pub mod evaluator;
#[cfg(test)]
mod evaluator_tests;
/// Core interpreter for executing plan steps with an instruction pointer.
pub mod interpreter;
#[cfg(test)]
mod interpreter_tests;
/// Navigation utilities for resolving steps within nested plan structures.
pub mod navigator;
#[cfg(test)]
mod navigator_tests;
/// Execution lifecycle observer traits and implementations.
pub mod observer;
#[cfg(test)]
mod observer_tests;
/// Runtime state types used by the execution engine.
pub mod state;
#[cfg(test)]
mod state_tests;
