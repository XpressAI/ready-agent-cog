//! Parser that converts Python-like plan code into an `AbstractPlan` AST.

use rustpython_parser::{Mode, ast, parse};

use crate::error::{ReadyError, Result};
use crate::plan::AbstractPlan;

mod conditions;
mod expressions;
mod functions;
mod literals;
mod names;
mod statements;

#[cfg(test)]
mod tests;

pub(crate) use conditions::convert_condition;
pub(crate) use expressions::convert_expression;
pub(crate) use functions::find_main;
pub(crate) use literals::{
    constant_to_literal, expression_to_literal_value, extract_string_literal,
    extract_subscript_key, literal_dict,
};
pub(crate) use names::{expression_name, extract_name_target, statement_name};
pub(crate) use statements::convert_body;

/// Parses Python-like plan code into an `AbstractPlan` rooted at `def main():`.
///
/// The parser uses `rustpython_parser` and converts the Python AST into the plan's `Step` and `Expression` types.
pub fn parse_python_to_plan(code: &str, name: &str) -> Result<AbstractPlan> {
    let ast = parse(code, Mode::Module, "<plan>")
        .map_err(|error| ReadyError::PlanParsing(error.to_string()))?;
    let body = match ast {
        ast::Mod::Module(module) => module.body,
        _ => {
            return Err(ReadyError::PlanParsing(
                "Expected module AST when parsing plan code".to_string(),
            ));
        }
    };
    let main = find_main(&body)?;
    let steps = convert_body(&main.body)?;

    Ok(AbstractPlan {
        name: name.to_string(),
        description: String::new(),
        steps,
        code: code.to_string(),
    })
}
