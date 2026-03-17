//! Plan parsing (Python to AST) and static validation.

/// Parser for converting Python-like plan code into the internal plan AST.
pub mod parser;
/// Static validation utilities for checking plans before execution.
pub mod validator;
