//! High-level orchestration layer that combines planning and execution into end-to-end SOP workflows.

/// High-level plan execution utilities for running workflow plans.
pub mod executor;
#[cfg(test)]
mod executor_tests;
/// LLM-powered planning utilities for turning SOP text into executable plans.
pub mod planner;
#[cfg(test)]
mod planner_tests;

/// Re-exports the high-level workflow executor.
pub use executor::SopExecutor;
/// Re-exports the SOP planner for generating workflow plans.
pub use planner::SopPlanner;
