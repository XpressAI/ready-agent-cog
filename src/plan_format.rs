//! Pretty-printing helpers for turning plan AST values back into readable text.
//!
//! The formatter renders [`AbstractPlan`] and [`Expression`] values into a Python-like representation
//! that is useful for debugging, inspection, and display.

#[cfg(test)]
use crate::plan::ComparisonOperator;
use crate::plan::{AbstractPlan, Accessor, BranchKind, Expression, Step};

/// Converts an [`AbstractPlan`] into a human-readable,
/// Python-like string.
///
/// The output includes the plan name, optional description, rendered steps, and the
/// stored source code block when present. This is primarily used for inspection,
/// debugging, and presenting plans back to users.
pub fn format_plan(plan: &AbstractPlan) -> String {
    let mut sections = vec![format!("Plan: {}", plan.name)];

    if !plan.description.trim().is_empty() {
        sections.push(format!("\n{}", plan.description.trim()));
    }

    if !plan.steps.is_empty() {
        sections.push("\n--- Steps ---\n".to_string());
        sections.push(format_steps(&plan.steps, 0).join("\n"));
    }

    if !plan.code.is_empty() {
        sections.push("\n--- Code ---\n".to_string());
        sections.push(plan.code.clone());
    }

    format!("{}\n", sections.join("\n"))
}

/// Converts an [`Expression`] into its textual representation.
///
/// This mirrors the plan language's surface syntax closely enough for display and
/// debugging, including access paths, operators, and boolean expressions.
pub fn format_expression(expr: &Expression) -> String {
    match expr {
        Expression::Literal { value } => match serde_json::to_string(value) {
            Ok(text) => text,
            Err(_) => value.to_string(),
        },
        Expression::AccessPath {
            variable_name,
            accessors,
        } => {
            let mut result = variable_name.clone();
            for accessor in accessors {
                match accessor {
                    Accessor::Attribute(key) => {
                        result.push('.');
                        result.push_str(key);
                    }
                    Accessor::Key(key) => {
                        result.push('[');
                        result.push_str(
                            &serde_json::to_string(key).unwrap_or_else(|_| format!("\"{}\"", key)),
                        );
                        result.push(']');
                    }
                    Accessor::Index(index) => {
                        result.push('[');
                        result.push_str(&index.to_string());
                        result.push(']');
                    }
                }
            }
            result
        }
        Expression::BinaryExpression {
            operator,
            left,
            right,
        } => format!(
            "{} {} {}",
            format_expression(left),
            operator.as_str(),
            format_expression(right)
        ),
        Expression::Comparison {
            operator,
            left,
            right,
        } => format!(
            "{} {} {}",
            format_expression(left),
            operator.as_str(),
            format_expression(right)
        ),
        Expression::UnaryExpression { operator, operand } => {
            format!("{}{}", operator.as_str(), format_expression(operand))
        }
        Expression::ConcatExpression { parts } => parts
            .iter()
            .map(format_expression)
            .collect::<Vec<_>>()
            .join(" + "),
        Expression::Boolean { operator, operands } => operands
            .iter()
            .map(format_expression)
            .collect::<Vec<_>>()
            .join(&format!(" {} ", operator.as_str())),
        Expression::Not { operand } => format!("not {}", format_expression(operand)),
        Expression::DictExpression { entries } => {
            let pairs: Vec<String> = entries
                .iter()
                .map(|(k, v)| {
                    format!(
                        "{}: {}",
                        serde_json::to_string(k).unwrap_or_else(|_| format!("\"{}\"", k)),
                        format_expression(v)
                    )
                })
                .collect();
            format!("{{{}}}", pairs.join(", "))
        }
        Expression::ArrayExpression { elements } => {
            let items: Vec<String> = elements.iter().map(format_expression).collect();
            format!("[{}]", items.join(", "))
        }
    }
}

fn format_steps(steps: &[Step], indent: usize) -> Vec<String> {
    let prefix = "  ".repeat(indent);
    let mut lines = Vec::new();

    for step in steps {
        match step {
            Step::AssignStep { target, value } => lines.push(format!(
                "{}{} = {}",
                prefix,
                target,
                format_expression(value)
            )),
            Step::ToolStep {
                tool_id,
                arguments,
                output_variable,
            } => {
                let args = arguments
                    .iter()
                    .map(format_expression)
                    .collect::<Vec<_>>()
                    .join(", ");
                let call = format!("{}({})", tool_id, args);
                match output_variable {
                    Some(name) => lines.push(format!("{}{} = {}", prefix, name, call)),
                    None => lines.push(format!("{}{}", prefix, call)),
                }
            }
            Step::SwitchStep { branches } => {
                for (index, branch) in branches.iter().enumerate() {
                    let header = match (&branch.kind, &branch.condition) {
                        (BranchKind::Else, _) | (_, None) => format!("{}else:", prefix),
                        (BranchKind::If, Some(condition)) if index == 0 => {
                            format!("{}if {}:", prefix, format_expression(condition))
                        }
                        (BranchKind::ElseIf, Some(condition))
                        | (BranchKind::If, Some(condition)) => {
                            format!("{}elif {}:", prefix, format_expression(condition))
                        }
                    };
                    lines.push(header);
                    lines.extend(format_steps(&branch.steps, indent + 1));
                }
            }
            Step::LoopStep {
                iterable_variable,
                item_variable,
                body,
            } => {
                lines.push(format!(
                    "{}for {} in {}:",
                    prefix, item_variable, iterable_variable
                ));
                lines.extend(format_steps(body, indent + 1));
            }
            Step::WhileStep { condition, body } => {
                lines.push(format!("{}while {}:", prefix, format_expression(condition)));
                lines.extend(format_steps(body, indent + 1));
            }
            Step::UserInteractionStep {
                prompt,
                output_variable,
            } => {
                let call = format!(
                    "collect_user_input({})",
                    serde_json::to_string(prompt).unwrap_or_else(|_| format!("\"{}\"", prompt))
                );
                match output_variable {
                    Some(name) => lines.push(format!("{}{} = {}", prefix, name, call)),
                    None => lines.push(format!("{}{}", prefix, call)),
                }
            }
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{ConditionalBranch, Expression, LiteralValue};
    use serde_json::json;

    fn literal(value: serde_json::Value) -> LiteralValue {
        match value {
            serde_json::Value::Null => LiteralValue::Null,
            serde_json::Value::Bool(value) => LiteralValue::Bool(value),
            serde_json::Value::Number(number) => {
                if let Some(value) = number.as_i64() {
                    LiteralValue::Integer(value)
                } else {
                    LiteralValue::Float(number.as_f64().expect("finite json number"))
                }
            }
            serde_json::Value::String(value) => LiteralValue::String(value),
            serde_json::Value::Array(values) => {
                LiteralValue::Array(values.into_iter().map(literal).collect())
            }
            serde_json::Value::Object(values) => LiteralValue::Object(
                values
                    .into_iter()
                    .map(|(key, value)| (key, literal(value)))
                    .collect(),
            ),
        }
    }

    #[test]
    fn formats_assign_step() {
        let plan = AbstractPlan {
            name: "demo".to_string(),
            description: String::new(),
            code: String::new(),
            steps: vec![Step::AssignStep {
                target: "x".to_string(),
                value: Expression::Literal {
                    value: literal(json!("hello")),
                },
            }],
        };

        assert!(format_plan(&plan).contains("x = \"hello\""));
    }

    #[test]
    fn formats_tool_step_with_output() {
        let plan = AbstractPlan {
            name: "demo".to_string(),
            description: String::new(),
            code: String::new(),
            steps: vec![Step::ToolStep {
                tool_id: "fetch".to_string(),
                arguments: vec![Expression::AccessPath {
                    variable_name: "url".to_string(),
                    accessors: Vec::new(),
                }],
                output_variable: Some("result".to_string()),
            }],
        };

        assert!(format_plan(&plan).contains("result = fetch(url)"));
    }

    #[test]
    fn formats_tool_step_without_output() {
        let plan = AbstractPlan {
            name: "demo".to_string(),
            description: String::new(),
            code: String::new(),
            steps: vec![Step::ToolStep {
                tool_id: "send".to_string(),
                arguments: vec![Expression::AccessPath {
                    variable_name: "message".to_string(),
                    accessors: Vec::new(),
                }],
                output_variable: None,
            }],
        };

        assert!(format_plan(&plan).contains("send(message)"));
    }

    #[test]
    fn formats_switch_step_with_branches() {
        let plan = AbstractPlan {
            name: "demo".to_string(),
            description: String::new(),
            code: String::new(),
            steps: vec![Step::SwitchStep {
                branches: vec![
                    ConditionalBranch {
                        kind: BranchKind::If,
                        condition: Some(Expression::AccessPath {
                            variable_name: "active".to_string(),
                            accessors: Vec::new(),
                        }),
                        steps: vec![Step::ToolStep {
                            tool_id: "run".to_string(),
                            arguments: Vec::new(),
                            output_variable: None,
                        }],
                    },
                    ConditionalBranch {
                        kind: BranchKind::Else,
                        condition: None,
                        steps: vec![Step::ToolStep {
                            tool_id: "stop".to_string(),
                            arguments: Vec::new(),
                            output_variable: None,
                        }],
                    },
                ],
            }],
        };

        let formatted = format_plan(&plan);
        assert!(formatted.contains("if active:"));
        assert!(formatted.contains("else:"));
        assert!(formatted.contains("run()"));
        assert!(formatted.contains("stop()"));
    }

    #[test]
    fn formats_loop_step() {
        let plan = AbstractPlan {
            name: "demo".to_string(),
            description: String::new(),
            code: String::new(),
            steps: vec![Step::LoopStep {
                iterable_variable: "items".to_string(),
                item_variable: "item".to_string(),
                body: vec![Step::ToolStep {
                    tool_id: "process".to_string(),
                    arguments: vec![Expression::AccessPath {
                        variable_name: "item".to_string(),
                        accessors: Vec::new(),
                    }],
                    output_variable: None,
                }],
            }],
        };

        let formatted = format_plan(&plan);
        assert!(formatted.contains("for item in items:"));
        assert!(formatted.contains("process(item)"));
    }

    #[test]
    fn formats_while_step() {
        let plan = AbstractPlan {
            name: "demo".to_string(),
            description: String::new(),
            code: String::new(),
            steps: vec![Step::WhileStep {
                condition: Expression::AccessPath {
                    variable_name: "running".to_string(),
                    accessors: Vec::new(),
                },
                body: vec![Step::ToolStep {
                    tool_id: "tick".to_string(),
                    arguments: Vec::new(),
                    output_variable: None,
                }],
            }],
        };

        let formatted = format_plan(&plan);
        assert!(formatted.contains("while running:"));
        assert!(formatted.contains("tick()"));
    }

    #[test]
    fn format_expression_formats_access_paths_with_attribute_and_index_access() {
        let expr = Expression::AccessPath {
            variable_name: "file".to_string(),
            accessors: vec![
                crate::plan::Accessor::Attribute("metadata".to_string()),
                crate::plan::Accessor::Key("url".to_string()),
            ],
        };

        assert_eq!(format_expression(&expr), "file.metadata[\"url\"]");
    }

    #[test]
    fn format_plan_renders_assign_tool_switch_loop_while_and_user_interaction() {
        let plan = AbstractPlan {
            name: "demo".to_string(),
            description: "Summarize recent standups.".to_string(),
            steps: vec![
                Step::AssignStep {
                    target: "last_summary_date".to_string(),
                    value: Expression::Literal {
                        value: literal(json!("")),
                    },
                },
                Step::LoopStep {
                    iterable_variable: "files".to_string(),
                    item_variable: "file".to_string(),
                    body: vec![Step::SwitchStep {
                        branches: vec![ConditionalBranch {
                            kind: BranchKind::If,
                            condition: Some(Expression::Comparison {
                                operator: ComparisonOperator::GreaterThan,
                                left: Box::new(Expression::AccessPath {
                                    variable_name: "file".to_string(),
                                    accessors: Vec::new(),
                                }),
                                right: Box::new(Expression::AccessPath {
                                    variable_name: "last_summary_date".to_string(),
                                    accessors: Vec::new(),
                                }),
                            }),
                            steps: vec![Step::ToolStep {
                                tool_id: "post_summary".to_string(),
                                arguments: vec![Expression::AccessPath {
                                    variable_name: "file".to_string(),
                                    accessors: Vec::new(),
                                }],
                                output_variable: None,
                            }],
                        }],
                    }],
                },
                Step::WhileStep {
                    condition: Expression::AccessPath {
                        variable_name: "running".to_string(),
                        accessors: Vec::new(),
                    },
                    body: vec![Step::ToolStep {
                        tool_id: "tick".to_string(),
                        arguments: Vec::new(),
                        output_variable: None,
                    }],
                },
                Step::UserInteractionStep {
                    prompt: "Confirm?".to_string(),
                    output_variable: Some("confirmation".to_string()),
                },
            ],
            code: "def main():\n    for file in files:\n        post_summary(file)".to_string(),
        };

        let formatted = format_plan(&plan);

        assert!(formatted.contains("Plan: demo"));
        assert!(formatted.contains("Summarize recent standups."));
        assert!(formatted.contains("--- Steps ---"));
        assert!(formatted.contains("last_summary_date = \"\""));
        assert!(formatted.contains("for file in files:"));
        assert!(formatted.contains("if file > last_summary_date:"));
        assert!(formatted.contains("post_summary(file)"));
        assert!(formatted.contains("while running:"));
        assert!(formatted.contains("tick()"));
        assert!(formatted.contains("confirmation = collect_user_input(\"Confirm?\")"));
        assert!(formatted.contains("--- Code ---"));
        assert!(formatted.contains("def main():"));
    }
}
