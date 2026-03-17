use rustpython_parser::ast;

use crate::error::{ReadyError, Result};

pub(crate) fn find_main(suite: &ast::Suite) -> Result<&ast::StmtFunctionDef> {
    suite
        .iter()
        .find_map(|statement| match statement {
            ast::Stmt::FunctionDef(function) if function.name.as_str() == "main" => Some(function),
            _ => None,
        })
        .ok_or_else(|| {
            ReadyError::PlanParsing(
                "No main() function found in the provided code. The code must define a top-level 'def main(): ...' function.".to_string(),
            )
        })
}
