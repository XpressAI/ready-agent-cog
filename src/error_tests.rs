//! Tests for error classification and recovery logic.

use crate::error::ReadyError;

/// Tests for `ReadyError::is_recoverable()` method.
/// 
/// This method distinguishes between runtime execution errors (recoverable)
/// and structural errors like missing tools or invalid plans (unrecoverable).
mod error_classification {
    use super::*;

    #[test]
    fn test_execution_error_is_recoverable() {
        // Runtime execution errors are recoverable because the plan structure
        // is valid but execution failed (e.g., tool returned unexpected result)
        let error = ReadyError::Execution {
            step_index: Some(5),
            step_type: Some("tool_call".to_string()),
            message: "Tool returned unexpected result".to_string(),
        };
        assert!(error.is_recoverable());
    }

    #[test]
    fn test_tool_error_is_recoverable() {
        // Tool errors during execution are recoverable - the tool exists but
        // failed during execution, which can be handled by retry or fallback
        let error = ReadyError::Tool {
            tool_id: "send_email".to_string(),
            message: "Connection timeout".to_string(),
        };
        assert!(error.is_recoverable());
    }

    #[test]
    fn test_plan_parsing_error_is_unrecoverable() {
        // Plan parsing errors are unrecoverable - the plan itself is invalid
        // and cannot be fixed by runtime recovery
        let error = ReadyError::PlanParsing("Invalid syntax at line 5".to_string());
        assert!(!error.is_recoverable());
    }

    #[test]
    fn test_plan_validation_error_is_unrecoverable() {
        // Plan validation errors are unrecoverable - the plan failed semantic
        // validation and cannot execute
        let error = ReadyError::PlanValidation("Unknown tool reference".to_string());
        assert!(!error.is_recoverable());
    }

    #[test]
    fn test_tool_not_found_is_unrecoverable() {
        // Tool not found errors are unrecoverable - the tool doesn't exist
        // in the registry and cannot be invoked
        let error = ReadyError::ToolNotFound("nonexistent_tool".to_string());
        assert!(!error.is_recoverable());
    }

    #[test]
    fn test_evaluation_error_is_unrecoverable() {
        // Expression evaluation errors are unrecoverable - they indicate
        // invalid expressions in the plan that cannot be fixed at runtime
        let error = ReadyError::Evaluation("Cannot evaluate expression".to_string());
        assert!(!error.is_recoverable());
    }

    #[test]
    fn test_duplicate_tool_is_unrecoverable() {
        // Duplicate tool registration is a structural error, not recoverable
        let error = ReadyError::DuplicateTool("duplicate_tool".to_string());
        assert!(!error.is_recoverable());
    }

    #[test]
    fn test_llm_error_is_unrecoverable() {
        // LLM errors are unrecoverable at execution time - they occur during
        // planning, not execution
        let error = ReadyError::Llm("LLM API failed".to_string());
        assert!(!error.is_recoverable());
    }

    #[test]
    fn test_io_error_is_unrecoverable() {
        // I/O errors are unrecoverable - they indicate system-level failures
        let error = ReadyError::Io(std::io::Error::new(std::io::ErrorKind::Other, "test"));
        assert!(!error.is_recoverable());
    }
}
