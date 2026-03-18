use rustpython_parser::ast;

use crate::error::{ReadyError, Result};

pub(crate) fn extract_name_target(expr: &ast::Expr) -> Result<String> {
    match expr {
        ast::Expr::Name(name) => Ok(name.id.to_string()),
        other => Err(ReadyError::PlanParsing(format!(
            "assignment target must be a simple variable name, not a '{}' — only `x = ...` is allowed",
            expression_name(other)
        ))),
    }
}

pub(crate) fn expression_name(expr: &ast::Expr) -> &'static str {
    match expr {
        ast::Expr::BoolOp(_) => "boolean operator (and/or)",
        ast::Expr::NamedExpr(_) => "walrus operator (:=)",
        ast::Expr::BinOp(_) => "binary operator",
        ast::Expr::UnaryOp(_) => "unary operator",
        ast::Expr::Lambda(_) => "lambda expression",
        ast::Expr::IfExp(_) => "inline if expression (ternary)",
        ast::Expr::Dict(_) => "dict literal",
        ast::Expr::Set(_) => "set literal",
        ast::Expr::ListComp(_) => "list comprehension",
        ast::Expr::SetComp(_) => "set comprehension",
        ast::Expr::DictComp(_) => "dict comprehension",
        ast::Expr::GeneratorExp(_) => "generator expression",
        ast::Expr::Await(_) => "await expression",
        ast::Expr::Yield(_) => "yield expression",
        ast::Expr::YieldFrom(_) => "yield from expression",
        ast::Expr::Compare(_) => "comparison expression",
        ast::Expr::Call(_) => "function call",
        ast::Expr::FormattedValue(_) => "f-string value",
        ast::Expr::JoinedStr(_) => "f-string",
        ast::Expr::Constant(_) => "literal constant",
        ast::Expr::Attribute(_) => "attribute access",
        ast::Expr::Subscript(_) => "subscript access",
        ast::Expr::Starred(_) => "starred expression (*x)",
        ast::Expr::Name(_) => "variable name",
        ast::Expr::List(_) => "list literal",
        ast::Expr::Tuple(_) => "tuple literal",
        ast::Expr::Slice(_) => "slice expression",
    }
}

pub(crate) fn statement_name(stmt: &ast::Stmt) -> &'static str {
    match stmt {
        ast::Stmt::FunctionDef(_) => "def (nested function definition)",
        ast::Stmt::AsyncFunctionDef(_) => "async def (async function definition)",
        ast::Stmt::ClassDef(_) => "class definition",
        ast::Stmt::Return(_) => "return statement",
        ast::Stmt::Delete(_) => "del statement",
        ast::Stmt::Assign(_) => "assignment",
        ast::Stmt::TypeAlias(_) => "type alias",
        ast::Stmt::AugAssign(_) => "augmented assignment (+=, -=, ...)",
        ast::Stmt::AnnAssign(_) => "annotated assignment",
        ast::Stmt::For(_) => "for loop",
        ast::Stmt::AsyncFor(_) => "async for loop",
        ast::Stmt::While(_) => "while loop",
        ast::Stmt::If(_) => "if statement",
        ast::Stmt::With(_) => "with statement",
        ast::Stmt::AsyncWith(_) => "async with statement",
        ast::Stmt::Match(_) => "match/case statement",
        ast::Stmt::Raise(_) => "raise statement",
        ast::Stmt::Try(_) => "try/except block",
        ast::Stmt::TryStar(_) => "try/except* block",
        ast::Stmt::Assert(_) => "assert statement",
        ast::Stmt::Import(_) => "import statement",
        ast::Stmt::ImportFrom(_) => "from ... import statement",
        ast::Stmt::Global(_) => "global statement",
        ast::Stmt::Nonlocal(_) => "nonlocal statement",
        ast::Stmt::Expr(_) => "expression statement",
        ast::Stmt::Pass(_) => "pass statement",
        ast::Stmt::Break(_) => "break statement",
        ast::Stmt::Continue(_) => "continue statement",
    }
}
