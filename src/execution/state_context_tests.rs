//! Tests for state context methods used in error recovery.

use crate::execution::state::{ExecutionError, ExecutionState};

/// Tests for `ExecutionState::to_llm_context()` method.
///
/// This method converts execution state to a string suitable for LLM context,
/// truncating variables to avoid overloading the LLM.
mod execution_state_context {
    use super::*;

    #[test]
    fn test_to_llm_context_with_empty_state() {
        let state = ExecutionState::default();
        let context = state.to_llm_context(10);
        
        assert!(context.contains("Position (ip_path)"));
        assert!(context.contains("Variables"));
        assert!(context.contains("0 shown"));
    }

    #[test]
    fn test_to_llm_context_with_variables() {
        let mut state = ExecutionState::default();
        state.interpreter_state.variables.insert("var1".to_string(), serde_json::json!("value1"));
        state.interpreter_state.variables.insert("var2".to_string(), serde_json::json!("value2"));
        state.interpreter_state.variables.insert("var3".to_string(), serde_json::json!("value3"));
        
        let context = state.to_llm_context(10);
        
        assert!(context.contains("Position (ip_path)"));
        assert!(context.contains("Variables"));
        assert!(context.contains("3 shown"));
        assert!(context.contains("var1"));
        assert!(context.contains("var2"));
        assert!(context.contains("var3"));
    }

    #[test]
    fn test_to_llm_context_truncates_variables() {
        let mut state = ExecutionState::default();
        for i in 0..15 {
            state.interpreter_state.variables.insert(
                format!("var{}", i),
                serde_json::json!(format!("value{}", i))
            );
        }
        
        let context = state.to_llm_context(5); // Only show 5 variables
        
        assert!(context.contains("5 shown"));
        // Verify truncation happened - context should be much shorter than full output
        let full_context = state.to_llm_context(100);
        assert!(context.len() < full_context.len());
    }

    #[test]
    fn test_to_llm_context_with_ip_path() {
        let mut state = ExecutionState::default();
        state.interpreter_state.ip_path = vec![2, 3, 4];
        
        let context = state.to_llm_context(10);
        
        assert!(context.contains("[2, 3, 4]"));
    }
}

/// Tests for `ExecutionError::to_llm_context()` method.
///
/// This method converts execution error to a string suitable for LLM context,
/// truncating the message to avoid overloading the LLM.
mod execution_error_context {
    use super::*;

    #[test]
    fn test_to_llm_context_basic() {
        let error = ExecutionError {
            step_index: Some(5),
            step_type: Some("tool_call".to_string()),
            exception_type: "ToolError".to_string(),
            message: "Connection timeout".to_string(),
        };
        
        let context = error.to_llm_context(500);
        
        assert!(context.contains("Error at step 5"));
        assert!(context.contains("ToolError"));
        assert!(context.contains("Connection timeout"));
    }

    #[test]
    fn test_to_llm_context_with_none_step_index() {
        let error = ExecutionError {
            step_index: None,
            step_type: None,
            exception_type: "UnknownError".to_string(),
            message: "Something went wrong".to_string(),
        };
        
        let context = error.to_llm_context(500);
        
        assert!(context.contains("Error at step 0"));
        assert!(context.contains("UnknownError"));
        assert!(context.contains("Something went wrong"));
    }

    #[test]
    fn test_to_llm_context_truncates_message() {
        let long_message = "a".repeat(1000);
        let error = ExecutionError {
            step_index: Some(10),
            step_type: Some("tool_call".to_string()),
            exception_type: "LongError".to_string(),
            message: long_message.clone(),
        };
        
        let context = error.to_llm_context(100); // Only 100 chars
        
        assert!(context.len() < long_message.len());
        assert!(context.contains("Error at step 10"));
        assert!(context.contains("LongError"));
    }
}
