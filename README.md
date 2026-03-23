# ready

**AI agents that actually follow the script.**

Ready translates plain-English [Standard Operating Procedures](https://en.wikipedia.org/wiki/Standard_operating_procedure) into deterministic programs. The LLM makes one call to convert your SOP into a plan, then gets out of the way. A Rust interpreter runs everything from there.

## The Problem

The mainstream agent pattern (LLM reasoning in a loop, deciding what to do at every step) has three structural flaws:

1. **Security.** Every piece of data the LLM sees influences its next action. Malicious input can hijack the agent. It's remote code execution as a service.
2. **Reliability.** LLMs are stochastic. The same workflow may break after a model update. Long processes exhaust the context window.
3. **Speed & cost.** Every step requires a full LLM round-trip. Tasks a program handles in milliseconds take minutes and burn tokens.

Ready's insight: LLMs are good at translation. They make terrible runtimes. So translate once, then run the result without them.

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
\n# → Plan: standup_process
# → ...
# → --- Prefillable Inputs ---
# →   --input transcript_url=<value>  # Enter the transcript URL:
```

Run it:

```sh
ready run --plan standup_process_plan.json --tools shell-tools.json --plans-dir ./plans \
  --input transcript_url='"https://docs.google.com/document/d/..."'
# or skip the plan step entirely:
ready run --sop standup_process.txt --tools shell-tools.json --plans-dir ./plans \
  --input transcript_url='"https://docs.google.com/document/d/..."'
```

`read_file`, `read_google_doc`, and `post_to_slack` in this example are user-defined shell tools loaded from [`shell-tools.json`](src/tools/shell.rs). The only built-in tools are `delegate_to_large_language_model`, `extract_from_plaintext`, and `sort_list`.

When the planner emits [`collect_user_input`](src/workflow/planner.rs:53), it writes a planning-time pseudo-function into the generated Python. During execution, that pseudo-call becomes a [`Step::UserInteractionStep`](src/planning/parser/statements.rs:78) handled by the [`PlanInterpreter`](src/execution/interpreter.rs:20). You can inspect those prefillable inputs up front with [`ready inspect`](src/main.rs), then satisfy them ahead of time with [`--input NAME=VALUE`](src/main.rs:90). Any remaining interaction still **suspends** and resumes exactly where it left off.

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
    // Obtain the value from a UI, API, email, whatever fits your system
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

The [`SopPlanner`](src/workflow/planner.rs) sends your SOP and tool signatures to the LLM, which generates Python restricted to a small subset: assignments, tool calls, `if`/`elif`/`else`, `for`, `while`, and simple expressions. Imports, classes, exceptions, and method calls are all excluded.

The [`parser`](src/planning/parser/mod.rs) converts this Python AST (via [rustpython-parser](https://crates.io/crates/rustpython-parser)) into an [`AbstractPlan`](src/plan/types.rs), a JSON-serializable intermediate representation. The [`validator`](src/planning/validator/mod.rs) performs static analysis: undefined variables, unknown tools, scope violations. If validation fails, the planner retries with the error message (up to 3 attempts).

The [`PlanInterpreter`](src/execution/interpreter.rs) runs the plan step-by-step: evaluating expressions, calling tools, managing control flow. Pure determinism. The LLM is already done.

## Key Concepts

**Plans are programs.** An [`AbstractPlan`](src/plan/types.rs) supports assignments, tool calls, conditionals, for/while loops, string concatenation, arithmetic, and nested attribute/index access. It's a program in a small, purpose-built language.

**Suspend and resume.** When a tool needs human input or wants to pause, execution records a serializable [`ExecutionState`](src/execution/state.rs:159) and suspends. Hours or days later, you provide the input and execution resumes at the exact instruction pointer.

**Plans as tools.** Plans can be [composed](src/tools/process.rs). One plan calls another as a tool.

**Shell tools.** Define external tools as [command templates in JSON](src/tools/shell.rs). Ready interpolates arguments and parses output (raw text, JSON, int, float, bool). You write JSON; skip the Rust.

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

**Inspect before execute.** Plans are reviewable JSON artifacts. [`ready inspect`](src/main.rs) them, diff them, version-control them. Every decision is visible before a single tool fires.

### Shell tools with JSON output

When a tool's stdout is a JSON object or array, set [`output_parsing`](src/tools/shell.rs:39) to `"json"`. Ready passes the raw stdout through `serde_json::from_str` and returns the parsed `Value` directly to the plan interpreter. The command must exit with code 0; a non-zero exit code produces a [`ReadyError::Tool`](src/error.rs) before any parsing is attempted.

Declare the shape of the JSON object in [`returns.fields`](src/tools/models.rs:23). Each entry in `fields` becomes a typed attribute in the generated Python class stub that the planner sees, so the LLM knows which keys to access in the plan.

For arrays of structured objects, keep using `fields` for the element shape and set [`type_name`](src/tools/models.rs:11) to `list[ElementType]`. The runtime already parses arbitrary JSON, and the planner stub generator now emits the element class while preserving the list type in the field or return annotation.

```json
{
  "get_weather": {
    "description": "Fetch current weather for a city",
    "arguments": [
      {
        "name": "city",
        "description": "City name",
        "type_name": "str"
      }
    ],
    "template": ["python", "tools/get_weather.py", "{city}"],
    "returns": {
      "description": "Current weather data",
      "type_name": "WeatherResult",
      "fields": [
        { "name": "temperature_c", "description": "Temperature in Celsius", "type_name": "float", "fields": [] },
        { "name": "condition",     "description": "Sky condition",          "type_name": "str",   "fields": [] },
        { "name": "humidity_pct",  "description": "Relative humidity 0–100","type_name": "int",   "fields": [] }
      ]
    },
    "output_parsing": "json",
    "active": true,
    "output_schema": null
  }
}
```

The planner receives this Python stub:

```python
class WeatherResult:
    temperature_c: float  # Temperature in Celsius
    condition: str        # Sky condition
    humidity_pct: int     # Relative humidity 0–100

def get_weather(city: str) -> WeatherResult:
    """Fetch current weather for a city"""
    ...
```

The plan can then access fields by attribute: `weather.temperature_c`, `weather.condition`, etc.

Array-of-objects outputs work the same way:

```json
{
  "get_latest_transcripts": {
    "description": "Fetch the latest meeting transcripts",
    "arguments": [],
    "template": ["python", "tools/get_latest_transcripts.py"],
    "returns": {
      "description": "Latest transcript results",
      "type_name": "LatestTranscriptsResult",
      "fields": [
        {
          "name": "transcripts",
          "description": "Array of transcript metadata objects",
          "type_name": "list[TranscriptFile]",
          "fields": [
            { "name": "id", "description": "File ID", "type_name": "str", "fields": [] },
            { "name": "name", "description": "File name", "type_name": "str", "fields": [] },
            { "name": "mimeType", "description": "MIME type", "type_name": "str", "fields": [] }
          ]
        }
      ]
    },
    "output_parsing": "json",
    "active": true,
    "output_schema": null
  }
}
```

The planner then sees:

```python
class TranscriptFile:
    id: str        # File ID
    name: str      # File name
    mimeType: str  # MIME type

class LatestTranscriptsResult:
    transcripts: list[TranscriptFile]  # Array of transcript metadata objects

def get_latest_transcripts() -> LatestTranscriptsResult:
    """Fetch the latest meeting transcripts"""
    ...
```

[`output_schema`](src/tools/shell.rs:40) accepts a JSON Schema object or `null`. The field is stored on [`ShellToolEntry`](src/tools/shell.rs:32) but is not validated at runtime; it is reserved for future use.

**Observer pattern.** Hook into execution via [`ExecutionObserver`](src/execution/observer.rs) to log, trace, or react to step start, completion, suspension, and errors.

## Conventional Agents vs. Ready

| | Conventional | Ready |
|---|---|---|
| LLM role | Runtime reasoning (every step) | One-time translator |
| Execution | Stochastic, LLM-in-the-loop | Deterministic interpreter |
| Loops | Re-engage LLM per iteration | Native loops, no LLM needed |
| Auditability | Buried in conversation history | Plan inspectable before run |
| Scale | Context window limits | [1M+ interpreter steps](benches/hanoi_stress.rs) proven |
| Security | Every input influences decisions | LLM only sees SOP at plan time |

## CLI Reference

```
ready plan    --sop <file> [--output <file>] [--tools <file>] [--plans-dir <dir>] [--model <name>]
ready run     [--sop <file>] [--plan <file>] [--tools <file>] [--plans-dir <dir>] [--model <name>] [--input NAME=VALUE ...]
ready inspect --plan <file>
ready tools   [--tools <file>] [--plans-dir <dir>]
```

Use [`ready inspect`](src/main.rs) to see which `NAME` values are actually prefillable for a given plan before calling [`ready run`](src/main.rs) with [`--input NAME=VALUE`](src/main.rs:90). Values are parsed as JSON when possible, otherwise treated as plain strings.

`--tools` points to a [`shell-tools.json`](src/tools/shell.rs) file. If omitted, Ready looks for `shell-tools.json` in the current directory.

`--plans-dir` points to a directory of saved `*_plan.json` files. Ready loads them via [`ProcessToolsModule`](src/tools/process.rs) and registers each one as a callable tool. Any saved plan in that directory becomes available by name inside other plans.

```sh
ready tools --tools shell-tools.json --plans-dir ./plans
```

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
