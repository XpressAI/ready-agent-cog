# ready

**AI agents that run like software, not like improv theater.**

Ready translates plain-English [Standard Operating Procedures](https://en.wikipedia.org/wiki/Standard_operating_procedure) into deterministic programs, then executes them without an LLM in the loop. The LLM's only job is translation — one call to turn your SOP into a plan. A Rust interpreter handles the rest.

## The Problem

The mainstream agent pattern — an LLM reasoning in a loop, deciding what to do next at every step — has three structural flaws:

1. **Security.** Every piece of data the LLM sees influences its next action. Malicious input can hijack the agent. It's remote code execution as a service.
2. **Reliability.** LLMs are stochastic. The same workflow may break after a model update. Long processes exhaust the context window.
3. **Speed & cost.** Every step requires a full LLM round-trip. Tasks a program handles in milliseconds take minutes and burn tokens.

Ready's insight: LLMs are excellent *translators* (description → code), terrible *runtimes*. Use translation once, then execute deterministically.

## Usage

Write an SOP:

```text
# standup_process.txt
1. Read the jargon file to understand the team vocabulary
2. Get the meeting transcript URL from the user
3. Fetch the transcript from Google Docs
4. Summarize the transcript using the jargon as context
5. Post the summary to Slack
```

Generate a plan:

```sh
ready plan --sop standup_process.txt --tools shell-tools.json --plans-dir ./plans
# → Saved plan to standup_process_plan.json
```

The LLM translates your SOP into constrained Python, which Ready parses into a JSON plan:

```python
def main():
    jargon = read_file("./jargon.txt")
    transcript_url = collect_user_input("Enter the transcript URL:")
    transcript = read_google_doc(transcript_url)
    summary = delegate_to_large_language_model(
        "Summarize this meeting using these terms:\n" + jargon,
        transcript
    )
    post_to_slack(summary)
```

Inspect it before running:

```sh
ready inspect --plan standup_process_plan.json
```

Run it:

```sh
ready run --plan standup_process_plan.json --tools shell-tools.json --plans-dir ./plans
# or skip the plan step entirely:
ready run --sop standup_process.txt --tools shell-tools.json --plans-dir ./plans
```

`read_file`, `read_google_doc`, and `post_to_slack` in this example are user-defined shell tools loaded from [`shell-tools.json`](src/tools/shell.rs). The only built-in tools are `delegate_to_large_language_model`, `extract_from_plaintext`, and `sort_list`.

When the planner emits [`collect_user_input`](src/workflow/planner.rs:158), it is writing a planning-time pseudo-function into the generated Python, not calling a runtime builtin tool. During parsing and execution, that pseudo-call becomes a [`Step::UserInteractionStep`](src/planning/parser/statements.rs:78) handled directly by the [`PlanInterpreter`](src/execution/interpreter.rs:315), which **suspends**, waits for the user to provide the URL, then resumes exactly where it stopped — even if that's hours later.

## Installation

```sh
cargo install --path .
```

Requires an OpenAI-compatible API:

```sh
export OPENAI_API_KEY="sk-..."
```

| Variable         | Default | Purpose |
|------------------|---|---|
| `OPENAI_API_KEY` | — | API authentication (required) |
| `READY_MODEL`    | `gpt-4o` | Which model to use for planning |
| `READY_API_BASE` | `https://api.openai.com/v1` | API endpoint (swap in any OpenAI-compatible server) |

## Library Usage

Ready is both a CLI tool and a Rust library. For local development, add it as a path dependency:

```toml
[dependencies]
ready = { path = "path/to/ready" }
tokio = { version = "1", features = ["full"] }
serde_json = "1"
```

### Create and execute a plan

```rust
use std::collections::HashMap;
use std::sync::Arc;
use ready::execution::observer::LoggingObserver;
use ready::execution::state::ExecutionStatus;
use ready::llm::client::OpenAiClient;
use ready::tools::{BuiltinToolsModule, InMemoryToolRegistry, ShellToolStore, ShellToolsModule};
use ready::workflow::{SopExecutor, SopPlanner};

#[tokio::main]
async fn main() -> ready::Result<()> {
    let llm = Arc::new(OpenAiClient::new(None, None, None));

    // Build a tool registry
    let mut registry = InMemoryToolRegistry::new();
    registry.register_module(Box::new(BuiltinToolsModule::new(llm.clone())));
    let shell_tools = ShellToolStore::load("shell-tools.json")?;
    if !shell_tools.is_empty() {
        registry.register_module(Box::new(ShellToolsModule::new(shell_tools)));
    }
    let registry = Arc::new(registry);

    // Generate a plan from an SOP
    let sop = std::fs::read_to_string("standup_process.txt")?;
    let planner = SopPlanner::new(llm, 3);
    let plan = planner.plan(&sop, registry.tools()).await?;

    // Execute it
    let executor = SopExecutor::new(registry, Some(Arc::new(LoggingObserver)));
    let state = executor.execute(&plan, HashMap::new(), None).await?;
    assert_eq!(state.status, ExecutionStatus::Completed);
    Ok(())
}
```

### Parse a plan without an LLM

If you already have the constrained Python, skip the planner:

```rust
use ready::planning::parser::parse_python_to_plan;

let plan = parse_python_to_plan(r#"
def main():
    data = read_file("report.txt")
    post_to_slack(data)
"#, "post_report")?;
```

### Suspend and resume

Plans that need human input suspend automatically. The public [`ExecutionState`](src/execution/state.rs:159) is serializable, so you can persist it and resume later:

```rust
use serde_json::json;
use ready::execution::state::ExecutionStatus;

let mut state = executor.execute(&plan, HashMap::new(), None).await?;

while state.status == ExecutionStatus::Suspended {
    let prompt = state.suspension_reason.as_deref().unwrap_or("Input needed");
    // Obtain the value from a UI, API, email — whatever fits your system
    let value = json!("user-provided-answer");
    executor.resume(&plan, &mut state, value).await?;
}
```

Or handle it synchronously with a callback:

```rust
executor.execute(&plan, HashMap::new(), Some(Box::new(|prompt: &str| {
    println!("{prompt}");
    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf).ok()?;
    Some(buf.trim().to_string())
}))).await?;
```

### Custom tools

Implement [`ToolsModule`](src/tools/traits.rs:1) to register your own tools:

```rust
use std::pin::Pin;
use std::future::Future;
use ready::{ToolDescription, ToolReturnDescription, ToolResult, ToolsModule};
use ready::tools::models::ToolCall;
use serde_json::Value;

struct MyModule;

impl ToolsModule for MyModule {
    fn tools(&self) -> &[ToolDescription] {
        static TOOLS: once_cell::sync::Lazy<Vec<ToolDescription>> = once_cell::sync::Lazy::new(|| {
            vec![ToolDescription {
                id: "my_tool".into(),
                name: "my_tool".into(),
                description: "Does something useful".into(),
                arguments: vec![],
                returns: ToolReturnDescription {
                    name: Some("output".into()),
                    description: "The result".into(),
                    type_name: Some("str".into()),
                    fields: vec![],
                },
            }]
        });
        &TOOLS
    }

    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
    ) -> Pin<Box<dyn Future<Output = ready::Result<ToolResult>> + Send + 'a>> {
        Box::pin(async move {
            Ok(ToolResult::Success(Value::String("done".into())))
        })
    }
}
```

Register it like any other module:

```rust
registry.register_module(Box::new(MyModule));
```

## How It Works

```
SOP (plain English)
  → LLM translates to constrained Python
    → Parser converts to AbstractPlan (JSON IR)
      → Validator catches errors statically
        → Interpreter executes deterministically
```

The [`SopPlanner`](src/workflow/planner.rs) sends your SOP and tool signatures to the LLM, which generates Python restricted to a small subset: assignments, tool calls, `if`/`elif`/`else`, `for`, `while`, and simple expressions. No imports, no classes, no exceptions, no method calls.

The [`parser`](src/planning/parser/mod.rs) converts this Python AST (via [rustpython-parser](https://crates.io/crates/rustpython-parser)) into an [`AbstractPlan`](src/plan/types.rs) — a JSON-serializable intermediate representation. The [`validator`](src/planning/validator/mod.rs) performs static analysis: undefined variables, unknown tools, scope violations. If validation fails, the planner retries with the error message (up to 3 attempts).

The [`PlanInterpreter`](src/execution/interpreter.rs) then runs the plan step-by-step. It evaluates expressions, calls tools, manages control flow — all deterministically. The LLM is not involved.

## Key Concepts

**Plans are programs.** An [`AbstractPlan`](src/plan/types.rs) supports assignments, tool calls, conditionals, for/while loops, string concatenation, arithmetic, and nested attribute/index access. It's a program in a small, purpose-built language.

**Suspend and resume.** When a tool needs human input or wants to pause, execution records a serializable [`ExecutionState`](src/execution/state.rs:159) and suspends. Hours or days later, you provide the input and execution resumes at the exact instruction pointer.

**Plans as tools.** Plans can be [composed](src/tools/process.rs) — one plan can call another plan as a tool, enabling complex workflows built from simple building blocks.

**Shell tools.** Define external tools as [command templates in JSON](src/tools/shell.rs) — no Rust code needed. Ready interpolates arguments and parses output (raw text, JSON, int, float, bool).

Minimal [`shell-tools.json`](src/tools/shell.rs) example:

```json
{
  "read_file": {
    "description": "Read a UTF-8 text file from disk",
    "arguments": [
      {
        "name": "path",
        "description": "Path to the file to read",
        "type_name": "str"
      }
    ],
    "template": ["python", "tools/read_file.py", "{path}"],
    "returns": {
      "description": "File contents as plain text",
      "type_name": "str"
    },
    "output_parsing": "raw",
    "active": true,
    "output_schema": null
  }
}
```

**Inspect before execute.** Plans are reviewable JSON artifacts. You can [`ready inspect`](src/main.rs) them, diff them, version-control them. They're not opaque conversation logs.

**Observer pattern.** Hook into execution via [`ExecutionObserver`](src/execution/observer.rs) to log, trace, or react to step start, completion, suspension, and errors.

## Conventional Agents vs. Ready

| | Conventional | Ready |
|---|---|---|
| LLM role | Runtime reasoning (every step) | One-time translator |
| Execution | Stochastic, LLM-in-the-loop | Deterministic interpreter |
| Loops | Re-engage LLM per iteration | Native — no LLM needed |
| Auditability | Buried in conversation history | Plan inspectable before run |
| Scale | Context window limits | [1M+ interpreter steps](benches/hanoi_stress.rs) proven |
| Security | Every input influences decisions | LLM only sees SOP at plan time |

## CLI Reference

```
ready plan    --sop <file> [--output <file>] [--tools <file>] [--plans-dir <dir>] [--model <name>]
ready run     [--sop <file>] [--plan <file>] [--tools <file>] [--plans-dir <dir>] [--model <name>]
ready inspect --plan <file>
ready tools   [--tools <file>] [--plans-dir <dir>]
```

`--tools` points to a [`shell-tools.json`](src/tools/shell.rs) file. If omitted, Ready looks for `shell-tools.json` in the current directory.

`--plans-dir` points to a directory of saved `*_plan.json` files. When provided, Ready loads those plans through [`ProcessToolsModule`](src/tools/process.rs) and registers each saved plan as a callable tool. This allows plans to invoke other pre-built plans as tools, giving you reusable sub-plans without reintroducing an LLM into runtime execution.

```sh
ready tools --tools shell-tools.json --plans-dir ./plans
```

Use [`--plans-dir`](src/main.rs) when you want plan composition: any saved plan in that directory becomes available by its plan name inside other plans.

## Project Structure

```
src/
├── main.rs              CLI entry point
├── lib.rs               Public API surface
├── plan/                AbstractPlan, Step, Expression types
├── plan_format.rs       Pretty-print plans as Python-like code
├── error.rs             ReadyError enum
├── execution/           Deterministic plan interpreter
├── llm/                 OpenAI-compatible HTTP client
├── planning/            SOP → Plan (parser + validator)
├── tools/               Tool system (builtins, shell, process, registry)
└── workflow/            High-level orchestration (SopPlanner, SopExecutor)
```

## Built With

[Rust](https://www.rust-lang.org/) (edition 2024) · [tokio](https://tokio.rs/) · [rustpython-parser](https://crates.io/crates/rustpython-parser) · [clap](https://docs.rs/clap) · [serde](https://serde.rs/) · [reqwest](https://docs.rs/reqwest)

Built by [XpressAI](https://xpress.ai).
