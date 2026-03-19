//! LLM-powered plan generation from Standard Operating Procedure (SOP) text.

use std::sync::Arc;
use tracing::{debug, trace, warn};

use crate::error::{ReadyError, Result};
use crate::llm::client::strip_markdown_fences;
use crate::llm::traits::LlmClient;
use crate::plan::{AbstractPlan, DiagnosticSeverity};
use crate::planning::parser::parse_python_to_plan;
use crate::planning::validator::validate_plan;
use crate::tools::models::{ToolDescription, generate_prompt_stubs};

const SYSTEM_TEMPLATE: &str = concat!(
    "You are an expert python developer. You are known for your ability to create code that is as simple as it gets.\n",
    "Your task is to translate a Standard Operating Procedure (SOP) into a valid Python function named `main` that calls the provided tool functions.\n\n",
    "But there is a twist: You only have a limited set of functionality available, as your code will be run on a new",
    "experimental interpreter. For this reason you may not use any built-in methods, or list comprehension. Not even the",
    "print method is available. Exceptions are also not available.\n\n",
    "Rules:\n",
    "- Your output MUST be a single Python function named `main` with no arguments.\n",
    "- Output Python code only. Do not wrap it in markdown fences.\n",
    "- Do NOT add any import statements.\n",
    "- Do NOT call any function that is not listed below.\n",
    "- Do NOT define helper functions, classes, lambdas, or nested functions.\n",
    "- Do NOT use try/except, with-statements, comprehensions, generator expressions, match/case, decorators, async/await, yield, break, continue, raise, assert, del, global, nonlocal, walrus operator, or chained method calls.\n",
    "- A tool call must be either used directly as a statement like `post_to_slack(message)` or assigned to a variable like `messages = get_slack_messages(channel)`.\n",
    "- Assignments may only target a single variable name on the left side.\n",
    "- Allowed statements inside `main` are only:\n",
    "  1. single-variable assignments such as `x = ...`\n",
    "  2. bare tool-call statements such as `post_to_slack(message)`\n",
    "  3. `if` / `elif` / `else` blocks\n",
    "  4. `for item in items:` loops where both `item` and `items` are simple variable names\n",
    "  5. `while condition:` loops\n",
    "  6. `pass` and `return`\n",
    "- Allowed expressions are only:\n",
    "  1. literals such as strings, integers, floats, booleans, and None\n",
    "  2. variable references such as `message`\n",
    "  3. attribute access such as `file.name`\n",
    "  4. constant index access such as `msg[\"content\"]` or `items[0]`\n",
    "  5. string concatenation with `+` or f-strings\n",
    "  6. arithmetic with `+`, `-`, `*`, `/`, `//`, `%`, `**`\n",
    "  7. unary `+` and unary `-`\n",
    "  8. boolean conditions using variables/literals/access paths with comparisons, `and`, `or`, and `not`\n",
    "- Conditions must never contain function calls such as `if is_ready()`; call the tool first, store the result in a variable, and then test that variable.\n",
    "- When checking structured tool results, use attribute access or constant-key indexing only.\n",
    "  Example: `content = msg[\"content\"]` is allowed.\n",
    "- Never use `.get(...)`, `.lower()`, `.append()`, `.sort()`, or any other method call on values.\n",
    "- If a transformation would require an unsupported feature, split it into simpler supported tool calls and assignments, or leave the value as-is.\n",
    "- Only use provided tool functions as function calls.\n",
    "- No imports, no class definitions, no decorators, no comprehensions.\n",
    "- Only assignments, tool calls, if/elif/else, for loops, while loops.\n",
    "- Use collect_user_input(\"prompt\") for user interaction.\n",
    "- All code must be inside `def main():`.\n",
    "- Use string concatenation with + for building strings.\n",
    "- F-strings are allowed.\n",
    "- Use only the following provided functions:\n\n",
    "{tool_stubs}"
);

const DESCRIPTION_SYSTEM: &str = concat!(
    "You are a workflow documentation assistant.\n",
    "Given a Standard Operating Procedure (SOP) and the list of input variables that a user must supply before the plan can run, write a concise one-paragraph description of what the plan does.\n\n",
    "Rules:\n",
    "- Be concise — aim for 2-4 sentences.\n",
    "- Mention the purpose of the plan.\n",
    "- If there are prefillable input variables, list them by name and their prompt so the caller knows what to prepare.\n",
    "- Do NOT include any code or markdown formatting — plain prose only."
);

const ERROR_SUFFIX_TEMPLATE: &str = concat!(
    "[Previous attempt failed — error: {error}]\n",
    "Broken code from the previous attempt:\n",
    "{code}\n\n",
    "Please fix the code and try again."
);

/// Orchestrates LLM-based plan generation from SOP text into a validated [`AbstractPlan`](src/plan.rs:1).
/// It generates Python plan code via the LLM, parses it, validates the result, and retries on failure.
pub struct SopPlanner {
    llm: Arc<dyn LlmClient>,
    max_retries: usize,
}

struct PlanGenerationArtifacts {
    system_prompt: String,
    initial_user_prompt: String,
    plan_name: String,
}

impl SopPlanner {
    /// Constructs a planner with an [`Arc`](src/workflow/planner.rs:1)-wrapped [`LlmClient`](src/llm/traits.rs:1) and a retry limit.
    pub fn new(llm: Arc<dyn LlmClient>, max_retries: usize) -> Self {
        Self { llm, max_retries }
    }

    /// Generates a validated [`AbstractPlan`](src/plan.rs:1) from SOP text and available tool descriptions.
    /// It prompts the LLM for Python plan code, parses and validates the result, and retries on parse or validation failures.
    pub async fn plan(
        &self,
        sop_text: &str,
        tool_descriptions: &[ToolDescription],
    ) -> Result<AbstractPlan> {
        let artifacts = build_plan_generation_artifacts(sop_text, tool_descriptions);
        let mut user_prompt = artifacts.initial_user_prompt.clone();
        let mut last_error: Option<ReadyError> = None;

        debug!(
            plan_name = %artifacts.plan_name,
            retries = self.max_retries,
            tool_count = tool_descriptions.len(),
            "starting SOP planning"
        );

        for attempt in 0..=self.max_retries {
            debug!(attempt = attempt + 1, max_attempts = self.max_retries + 1, "requesting plan generation from LLM");
            trace!(system_prompt = artifacts.system_prompt.as_str(), user_prompt = user_prompt.as_str(), "planner prompts prepared");

            let raw = self
                .llm
                .complete(&artifacts.system_prompt, &user_prompt)
                .await?;
            let code = strip_markdown_fences(&raw);

            trace!(raw_response = raw.as_str(), stripped_code = code.as_str(), "planner received candidate plan code");

            let plan = match parse_and_validate_plan(&code, &artifacts.plan_name, tool_descriptions)
            {
                Ok(plan) => plan,
                Err(error) => {
                    warn!(attempt = attempt + 1, error = %error, "planner rejected candidate plan; preparing retry if available");
                    last_error = Some(error);
                    if attempt < self.max_retries {
                        user_prompt = build_retry_prompt(
                            sop_text,
                            &code,
                            last_error.as_ref().expect("error set"),
                        );
                        trace!(retry_prompt = user_prompt.as_str(), "planner retry prompt prepared");
                        continue;
                    }
                    return Err(last_error.expect("error set"));
                }
            };

            let mut plan = plan;
            let description_prompt = build_description_prompt(sop_text, &plan.prefillable_inputs());
            trace!(description_prompt = description_prompt.as_str(), "requesting plan description from LLM");
            plan.description = self
                .llm
                .complete(DESCRIPTION_SYSTEM, &description_prompt)
                .await?
                .trim()
                .to_string();
            debug!(plan_name = %plan.name, step_count = plan.steps.len(), "planning completed successfully");
            return Ok(plan);
        }

        Err(last_error.unwrap_or_else(|| {
            ReadyError::PlanValidation("SopPlanner exhausted retries without a result".to_string())
        }))
    }
}

fn build_plan_generation_artifacts(
    sop_text: &str,
    tool_descriptions: &[ToolDescription],
) -> PlanGenerationArtifacts {
    PlanGenerationArtifacts {
        system_prompt: build_system_prompt(tool_descriptions),
        initial_user_prompt: sop_text.to_string(),
        plan_name: infer_plan_name(sop_text),
    }
}

pub(crate) fn parse_and_validate_plan(
    code: &str,
    plan_name: &str,
    tool_descriptions: &[ToolDescription],
) -> Result<AbstractPlan> {
    trace!(plan_name = plan_name, code = code, "parsing generated plan code");
    let plan = parse_python_to_plan(code, plan_name)?;
    let hard_errors = validation_errors(&plan, tool_descriptions);

    if hard_errors.is_empty() {
        Ok(plan)
    } else {
        warn!(plan_name = plan_name, errors = ?hard_errors, "plan validation produced hard errors");
        Err(ReadyError::PlanValidation(format!(
            "Plan validation failed: {}",
            hard_errors.join("; ")
        )))
    }
}

fn validation_errors(plan: &AbstractPlan, tool_descriptions: &[ToolDescription]) -> Vec<String> {
    validate_plan(plan, tool_descriptions)
        .iter()
        .filter(|issue| issue.severity == DiagnosticSeverity::Error)
        .map(|issue| issue.message.clone())
        .collect()
}

fn build_system_prompt(tool_descriptions: &[ToolDescription]) -> String {
    let mut descriptions = tool_descriptions.to_vec();
    descriptions.push(collect_user_input_description());
    SYSTEM_TEMPLATE.replace("{tool_stubs}", &generate_prompt_stubs(&descriptions))
}

fn collect_user_input_description() -> ToolDescription {
    ToolDescription {
        id: "collect_user_input".to_string(),
        description: "Collect a value from the user when the workflow requires human input."
            .to_string(),
        arguments: vec![crate::tools::models::ToolArgumentDescription {
            name: "prompt".to_string(),
            description: "Prompt shown to the user.".to_string(),
            type_name: "str".to_string(),
            default: None,
        }],
        returns: crate::tools::models::ToolReturnDescription {
            name: Some("output".to_string()),
            description: "User-provided input value.".to_string(),
            type_name: Some("str".to_string()),
            fields: Vec::new(),
        },
    }
}

pub(crate) fn build_retry_prompt(sop_text: &str, code: &str, error: &ReadyError) -> String {
    let suffix = ERROR_SUFFIX_TEMPLATE
        .replace("{error}", &error.to_string())
        .replace("{code}", code);
    format!("{sop_text}\n\n{suffix}")
}

pub(crate) fn build_description_prompt(
    sop_text: &str,
    prefillable: &[crate::plan::PrefillableInput],
) -> String {
    let mut lines = vec![format!("SOP:\n{sop_text}")];
    if prefillable.is_empty() {
        lines.push("\nThere are no prefillable input variables.".to_string());
    } else {
        lines.push("\nPrefillable input variables:".to_string());
        for item in prefillable {
            lines.push(format!("  - {}: \"{}\"", item.variable_name, item.prompt));
        }
    }
    lines.join("\n")
}

fn infer_plan_name(sop_text: &str) -> String {
    sop_text
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .unwrap_or_else(|| "generated_plan".to_string())
}
