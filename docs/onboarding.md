# Welcome to Ready: Your Onboarding Guide

## What Is Ready?

Ready is a **plan-based agent execution engine** that turns plain-English Standard Operating Procedures (SOPs) into deterministic programs. The LLM translates your SOP into a plan once, then gets out of the way. A Rust interpreter runs everything from there.

Think of it this way: **LLMs are good at translation. They make terrible runtimes.** Ready's insight is to translate once, then run the result without them.

### The Problem Ready Solves

Conventional AI agents use an LLM in a reasoning loop, deciding what to do at every step. This has three structural flaws:

1. **Security**: Every piece of data the LLM sees influences its next action. Malicious input can hijack the agent. It's remote code execution as a service.
2. **Reliability**: LLMs are stochastic. The same workflow may break after a model update. Long processes exhaust the context window.
3. **Speed & Cost**: Every step requires a full LLM round-trip. Tasks a program handles in milliseconds take minutes and burn tokens.

Ready's solution: **Translate once, execute deterministically.**

---

## Quick Tour: How It Works

```
SOP (plain English)
  → LLM translates to constrained Python
    → Parser converts to AbstractPlan (JSON IR)
      → Validator catches errors statically
        → Interpreter executes deterministically
```

### Example Flow

1. **You write an SOP** (plain English):
   ```text
   1. Read the jargon file to understand the team vocabulary
   2. Get the meeting transcript URL from the user
   3. Fetch the transcript from Google Docs
   4. Summarize the transcript using the jargon as context
   5. Post the summary to Slack
   ```

2. **Ready generates a plan** (LLM translates to constrained Python):
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

3. **Ready parses it** (Python → AbstractPlan AST):
   The parser converts this into an `AbstractPlan` — a JSON-serializable intermediate representation.

4. **Ready validates it** (static analysis):
   The validator checks for undefined variables, unknown tools, scope violations. If validation fails, the planner retries with the error message (up to 3 attempts).

5. **Ready executes it** (deterministic interpreter):
   The `PlanInterpreter` runs the plan step-by-step: evaluating expressions, calling tools, managing control flow. Pure determinism. The LLM is already done.

---

## Project Structure

```
src/
├── main.rs              # CLI entry point (plan, run, inspect, tools commands)
├── lib.rs               # Public API surface
├── error.rs             # ReadyError enum (centralized error types)
├── plan_format.rs       # Pretty-print plans as Python-like code
├── test_helpers.rs      # Shared test utilities
│
├── plan/                # The AST (AbstractPlan, Step, Expression types)
│   ├── mod.rs           # Module exports
│   ├── types.rs         # AbstractPlan struct
│   ├── step.rs          # Step enum (executable statements)
│   ├── expression.rs    # Expression enum (value representations)
│   ├── diagnostics.rs   # PlanDiagnostic types
│   └── queries.rs       # Query helpers for plan introspection
│
├── planning/            # Parser and Validator (SOP → Plan)
│   ├── parser/          # Python → AbstractPlan conversion
│   │   └── mod.rs       # parse_python_to_plan() using rustpython-parser
│   └── validator/       # Static analysis
│       └── mod.rs       # validate_plan() checks undefined vars, unknown tools
│
├── execution/           # Deterministic plan interpreter
│   ├── interpreter.rs   # PlanInterpreter: core execution engine
│   ├── evaluator.rs     # Expression evaluation helpers
│   ├── navigator.rs     # Step navigation within nested structures
│   ├── state.rs         # ExecutionState, InstructionPointer, InterpreterState
│   └── observer.rs      # ExecutionObserver trait for lifecycle hooks
│
├── tools/               # Tool system (builtins, shell, process, registry)
│   ├── traits.rs        # ToolsModule trait (extension point)
│   ├── models.rs        # ToolDescription, ToolCall, ToolResult
│   ├── registry/        # InMemoryToolRegistry
│   ├── builtin.rs       # Built-in tools (LLM delegation, extraction, sorting)
│   ├── shell.rs         # Shell-backed tools (JSON command templates)
│   └── process.rs       # Process-backed tools (plans as tools)
│
├── llm/                 # OpenAI-compatible HTTP client
│   ├── traits.rs        # LlmClient trait
│   └── client.rs        # OpenAiClient implementation
│
└── workflow/            # High-level orchestration
    ├── planner.rs       # SopPlanner: LLM-powered plan generation
    └── executor.rs      # SopExecutor: high-level plan execution
```

---

## Core Concepts

### 1. Plans Are Programs

An [`AbstractPlan`](src/plan/types.rs) is not a vague "intent" — it's a program in a small, purpose-built language. It supports:

- Assignments: `x = ...`
- Tool calls: `post_to_slack(message)` or `messages = get_slack_messages(channel)`
- Conditionals: `if` / `elif` / `else`
- Loops: `for item in items:` and `while condition:`
- Expressions: literals, variable access, attribute/index access, arithmetic, string concatenation, comparisons, boolean logic

**Why this matters**: Because plans are programs, they can be parsed, validated, and executed deterministically. No LLM involvement at runtime.

### 2. Suspend and Resume

When a tool needs human input or wants to pause, execution records a serializable [`ExecutionState`](src/execution/state.rs) and suspends. Hours or days later, you provide the input and execution resumes at the exact instruction pointer.

**Why this matters**: This enables human-in-the-loop workflows without losing state. The `ExecutionState` is JSON-serializable, so you can persist it to a database, send it over the network, or store it in a file.

### 3. Plans as Tools

Plans can be [composed](src/tools/process.rs). One plan calls another as a tool. This is the `ProcessToolsModule`.

**Why this matters**: You can build complex workflows from reusable sub-plans. A "send_slack_notification" plan can be called from a "handle_customer_complaint" plan. The sub-plan's user inputs become arguments to the tool call.

### 4. Shell Tools

Define external tools as [command templates in JSON](src/tools/shell.rs). Ready interpolates arguments and parses output (raw text, JSON, int, float, bool). You write JSON; skip the Rust.

**Why this matters**: Most tools are just shell commands. Instead of writing Rust wrappers for everything, you define them in JSON:

```json
{
  "read_file": {
    "description": "Read a UTF-8 text file from disk",
    "arguments": [
      { "name": "path", "description": "Path to the file", "type_name": "str" }
    ],
    "template": ["python", "tools/read_file.py", "{path}"],
    "returns": { "description": "File contents", "type_name": "str" },
    "output_parsing": "raw",
    "active": true
  }
}
```

### 5. Inspect Before Execute

Plans are reviewable JSON artifacts. [`ready inspect`](src/main.rs) them, diff them, version-control them. Every decision is visible before a single tool fires.

**Why this matters**: You can audit what the agent will do before it does it. This is critical for security and reliability.

---

## The Tool System

The tool system is Ready's extension point. Everything the agent can "do" is a tool.

### Tool Abstraction

The [`ToolsModule`](src/tools/traits.rs) trait defines the interface:

```rust
#[async_trait]
pub trait ToolsModule: Send + Sync {
    fn tools(&self) -> &[ToolDescription];
    async fn execute(&self, call: &ToolCall) -> Result<ToolResult>;
}
```

**Why this design**:
- `tools()` returns descriptions so the LLM knows what tools are available when generating plans.
- `execute()` runs the tool at runtime.
- The trait is `Send + Sync` for async execution.
- The return type is a boxed future for trait object compatibility.

### Tool Registry

The [`InMemoryToolRegistry`](src/tools/registry/runtime.rs) stores tool modules and provides:
- `register_module()`: Add a new tool module
- `tools()`: Get all tool descriptions (for the LLM)
- `execute()`: Run a tool by ID (for the interpreter)

**Why this design**:
- Modules are the unit of registration (not individual tools).
- The registry is the single source of truth for both planning and execution.
- Lookup is by tool ID for simplicity.

### Built-in Tools

The [`BuiltinToolsModule`](src/tools/builtin.rs) provides:
- `delegate_to_large_language_model`: Call an LLM for text generation
- `extract_from_plaintext`: Extract structured data from text
- `sort_list`: Sort a list of dictionaries

**Why these tools**:
- These are the only tools that require an LLM. They're built-in because they need access to the LLM client.
- Everything else (file I/O, HTTP requests, database queries) should be shell tools or custom modules.

### Shell Tools

The [`ShellToolsModule`](src/tools/shell.rs) executes templated commands and parses their output.

**Why shell tools**:
- Most tools are shell commands. This avoids writing Rust wrappers.
- JSON configuration is easy to version-control and share.
- Output parsing handles raw text, JSON, int, float, bool.

### Process Tools

The [`ProcessToolsModule`](src/tools/process.rs) exposes saved plans as callable tools.

**Why process tools**:
- Plans can call other plans. This enables composition.
- The sub-plan's user inputs become arguments.
- The sub-plan's execution state is serialized for suspend/resume.

---

## The Execution Engine

The execution engine is the heart of Ready. It runs plans deterministically.

### PlanInterpreter

The [`PlanInterpreter`](src/execution/interpreter.rs) is the core execution engine. It:
- Walks plan steps using an instruction pointer
- Evaluates expressions
- Dispatches tool calls
- Manages control flow (conditionals, loops)
- Handles suspend/resume

**Why this design**:
- The interpreter is a state machine. It maintains an instruction pointer and advances it step-by-step.
- Tool calls are async. The interpreter awaits them.
- Suspend/resume is built-in. The interpreter can pause and resume at any point.

### ExecutionState

The [`ExecutionState`](src/execution/state.rs) tracks runtime state:
- `status`: Pending, Running, Suspended, Completed, Failed
- `interpreter_state`: Variables, instruction pointer, pending continuations
- `error`: Execution error (if failed)
- `suspension_reason`: Why execution suspended (if suspended)

**Why this design**:
- The state is JSON-serializable. You can persist it and resume later.
- The instruction pointer is a path into the plan tree. It tracks nested scopes.
- Variables are a HashMap. Simple and flexible.

### Observer Pattern

The [`ExecutionObserver`](src/execution/observer.rs) trait provides lifecycle hooks:
- `on_plan_start()`: Before execution begins
- `on_step_start()`: Before each step
- `on_step_complete()`: After each step
- `on_suspension()`: When execution suspends
- `on_error()`: When an error occurs
- `on_plan_complete()`: After execution ends

**Why this design**:
- Observers are pluggable. You can log, trace, or react to execution events.
- The default observer is a no-op. Observers are opt-in.
- The `LoggingObserver` implementation uses `tracing` for structured logging.

---

## The Planning Pipeline

The planning pipeline turns SOP text into validated plans.

### SopPlanner

The [`SopPlanner`](src/workflow/planner.rs) orchestrates LLM-based plan generation:
1. Build a system prompt with tool stubs
2. Send SOP text to the LLM
3. Receive Python code
4. Parse to AbstractPlan
5. Validate the plan
6. If validation fails, retry with error message (up to 3 attempts)
7. If validation succeeds, generate a description
8. Return the plan

**Why this design**:
- The LLM is only used for translation. It never sees runtime data.
- Validation is static. Errors are caught before execution.
- Retry logic handles LLM mistakes. The LLM can self-correct.

### Parser

The [`parse_python_to_plan()`](src/planning/parser/mod.rs) function converts Python code to an AbstractPlan:
1. Parse Python with `rustpython-parser`
2. Convert Python AST to Ready AST
3. Return the AbstractPlan

**Why this design**:
- Python is familiar to most developers. The LLM can generate it reliably.
- The parser is constrained. Only a subset of Python is allowed (no imports, no classes, no exceptions).
- The AST is JSON-serializable. Plans can be stored and transmitted.

### Validator

The [`validate_plan()`](src/planning/validator/mod.rs) function performs static analysis:
1. Walk the plan tree
2. Track defined and used variables
3. Check for undefined variables
4. Check for unknown tools
5. Check for scope violations
6. Report diagnostics

**Why this design**:
- Validation is static. No execution required.
- Diagnostics are structured. They can be used for retry logic.
- The validator is extensible. New checks can be added.

---

## Why Rust?

Ready is written in Rust. Here's why:

### Type Safety

The AST and execution engine are heavily typed. Rust's type system catches errors at compile time.

### Performance

The interpreter runs millions of steps. Rust's performance is critical. The Hanoi stress test proves this: Ready can execute 1M+ interpreter steps without LLM involvement.

### Compile-Time Guarantees

Rust's borrow checker and ownership model prevent many classes of bugs. The execution engine is complex; compile-time guarantees are invaluable.

### Async/Await

Rust's async model is well-suited for tool execution. Tools can be async without blocking the interpreter.

---

## Why This Architecture?

### Separation of Planning and Execution

Planning (LLM translation) and execution (deterministic interpreter) are separate phases.

**Why**: This enables security, reliability, and speed. The LLM never sees runtime data. Execution is deterministic. No LLM round-trips during execution.

### Deterministic Execution

Once a plan is generated, it runs without LLM involvement.

**Why**: This enables reliability and auditability. The same plan always produces the same result. Plans are inspectable before execution.

### Suspend/Resume Model

Plans can pause for user input and resume later.

**Why**: This enables human-in-the-loop workflows. The execution state is serializable, so it can be persisted and resumed.

### Tool Modularity

Tools are pluggable via the `ToolsModule` trait.

**Why**: This enables extensibility. New tools can be added without modifying the core engine.

### Observer Pattern

Execution lifecycle hooks are pluggable.

**Why**: This enables observability. Logging, tracing, and monitoring can be added without modifying the core engine.

---

## Error Recovery

Ready's "LLM only at planning time" principle is elegant, but it creates a tension: what happens when the world doesn't cooperate with the plan? A tool call might fail because an API token has expired, a file has moved, or a remote service returns an unexpected response. The plan was valid when it was generated — the interpreter just ran into something it couldn't handle.

This is where error recovery comes in. Rather than abandoning the run entirely, Ready can ask the LLM to reason about what went wrong and generate a continuation plan that picks up where execution left off.

### The Two Kinds of Failure

Not all errors are created equal. Ready distinguishes between two fundamentally different failure modes, and the distinction matters enormously for recovery.

**Structural errors** — [`PlanParsing`](src/error.rs:17), [`PlanValidation`](src/error.rs:20), and [`ToolNotFound`](src/error.rs:29) — mean the plan itself is broken. The LLM generated code that couldn't be parsed, referenced a variable that doesn't exist, or called a tool that isn't registered. Retrying execution won't help; the plan needs to be regenerated from scratch. These errors are not recoverable.

**Runtime errors** — [`Execution`](src/error.rs:41) and [`Tool`](src/error.rs:25) — mean the plan was structurally sound but something went wrong while it was running. A tool returned an error, or the interpreter hit an unexpected condition. The plan's logic was fine; the environment wasn't. These errors *are* recoverable, because the LLM can look at what happened and reason about a different approach.

This classification lives in [`ReadyError::is_recoverable()`](src/error.rs:67). The logic is intentionally simple: if the error came from running the plan, recovery is worth attempting; if the error came from building the plan, it isn't.

### What Recovery Actually Does

When [`SopExecutor::execute_with_recovery()`](src/workflow/executor.rs:175) detects a failed execution with a recoverable error, it assembles a [`RecoveryContext`](src/execution/state.rs:266) — a bundle containing the original plan, the execution state at the point of failure, and the error details. This context is handed to [`SopPlanner::recover()`](src/workflow/planner.rs:186), which sends it to the LLM along with the original SOP text.

The LLM's job at this point is different from its job during initial planning. It isn't translating an SOP from scratch. It's reading a post-mortem: here is what the plan was trying to do, here is where it got to, here is what went wrong. From that, it generates a *continuation plan* — a new, complete, valid plan that handles the error and carries on with whatever work remains.

That continuation plan is then executed via [`SopExecutor::execute_from_checkpoint()`](src/workflow/executor.rs:227), which initialises execution from the failed state's variables and instruction pointer. Steps that already completed successfully don't run again.

### Keeping the LLM Prompt Manageable

One practical concern with passing execution state to an LLM is size. A long-running plan might accumulate dozens of variables, some of them large strings — full document contents, API responses, and so on. Dumping all of that into a prompt would be wasteful and potentially counterproductive.

[`ExecutionState::to_llm_context()`](src/execution/state.rs:226) addresses this by truncating: it takes only the first `max_vars` variables and caps error messages at `max_len` characters. The LLM gets enough context to understand what happened and where, without being overwhelmed by data it doesn't need.

### What the LLM Can Do With This

The recovery prompt gives the LLM genuine latitude to reason about the failure. Consider a few scenarios:

A plan fetches a document from a URL and the tool returns a 403. The LLM might generate a continuation that calls `collect_user_input` to ask the user for credentials, then retries the fetch. The original plan had no reason to anticipate an auth failure; the recovery plan can handle it gracefully.

A plan posts a message to a Slack channel and the tool reports the channel doesn't exist. The LLM might generate a continuation that asks the user to confirm the channel name, then retries. Or it might decide the step can be skipped and continue with the rest of the workflow.

A plan calls a shell tool that exits with a non-zero status. The LLM can read the error message, decide whether it's fatal, and either find a workaround or surface the problem to the user via `collect_user_input`.

In all of these cases, the LLM is doing what it's good at — reading context and reasoning about it — while the deterministic interpreter handles everything else.

### The Limits of Recovery

Recovery is not a magic safety net. It has a `max_recovery_attempts` ceiling, and if the LLM can't generate a valid continuation plan within the retry budget, the run fails. The recovery plan itself goes through the same parse-and-validate pipeline as any other plan, so a hallucinated tool call or a syntax error will be caught and retried, but eventually the budget runs out.

More fundamentally, recovery only works for errors the LLM can reason about. If a tool is consistently broken — returning garbage output regardless of how it's called — the LLM will keep generating plans that fail in the same way. Recovery is most useful for *situational* failures: auth problems, missing resources, unexpected but interpretable error messages.

### How This Fits the Architecture

Error recovery is a deliberate extension of the "LLM only at planning time" principle, not a violation of it. The LLM is still only invoked to generate plans — it never touches runtime data directly, never decides which tool to call next, never sees the full execution context. What changes is that "planning time" can now happen more than once per run: once at the start, and again if something goes wrong.

The suspend/resume model (discussed in [Core Concepts](#2-suspend-and-resume)) is the foundation that makes this possible. Because execution state is fully serializable, it can be handed to the planner as context and then resumed from a checkpoint. The two features compose naturally.

---

## Getting Started

### Build and Run

```bash
cargo build
cargo run -- plan --sop standup_process.txt --tools shell-tools.json --plans-dir ./plans
cargo run -- run --plan standup_process_plan.json --tools shell-tools.json --plans-dir ./plans
cargo run -- inspect --plan standup_process_plan.json
cargo run -- tools --tools shell-tools.json --plans-dir ./plans
```

### Library Usage

```rust
use std::sync::Arc;
use ready::execution::observer::LoggingObserver;
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

---

## Key Files to Know

| File | Purpose |
|------|---------|
| [`src/plan/types.rs`](src/plan/types.rs) | `AbstractPlan` struct — the core data model |
| [`src/plan/step.rs`](src/plan/step.rs) | `Step` enum — executable statements |
| [`src/plan/expression.rs`](src/plan/expression.rs) | `Expression` enum — value representations |
| [`src/planning/parser/mod.rs`](src/planning/parser/mod.rs) | `parse_python_to_plan()` — Python to AST |
| [`src/planning/validator/mod.rs`](src/planning/validator/mod.rs) | `validate_plan()` — static analysis |
| [`src/execution/interpreter.rs`](src/execution/interpreter.rs) | `PlanInterpreter` — core execution engine |
| [`src/execution/state.rs`](src/execution/state.rs) | `ExecutionState` — runtime state |
| [`src/execution/observer.rs`](src/execution/observer.rs) | `ExecutionObserver` — lifecycle hooks |
| [`src/tools/traits.rs`](src/tools/traits.rs) | `ToolsModule` — tool extension point |
| [`src/tools/registry/runtime.rs`](src/tools/registry/runtime.rs) | `InMemoryToolRegistry` — tool lookup |
| [`src/tools/builtin.rs`](src/tools/builtin.rs) | `BuiltinToolsModule` — built-in tools |
| [`src/tools/shell.rs`](src/tools/shell.rs) | `ShellToolsModule` — shell-backed tools |
| [`src/tools/process.rs`](src/tools/process.rs) | `ProcessToolsModule` — plans as tools |
| [`src/llm/traits.rs`](src/llm/traits.rs) | `LlmClient` — LLM abstraction |
| [`src/llm/client.rs`](src/llm/client.rs) | `OpenAiClient` — OpenAI implementation |
| [`src/workflow/planner.rs`](src/workflow/planner.rs) | `SopPlanner` — LLM-powered planning |
| [`src/workflow/executor.rs`](src/workflow/executor.rs) | `SopExecutor` — high-level execution |
| [`src/main.rs`](src/main.rs) | CLI entry point |

---

## Testing

Ready has extensive tests. Run them with:

```bash
cargo test
```

The Hanoi stress test (`benches/hanoi_stress.rs`) proves the interpreter can handle 1M+ steps without LLM involvement.

---

## Final Notes

Ready is a fundamentally different approach to AI agents. Instead of using LLMs at runtime, Ready uses them for translation only. The result is a system that is secure, reliable, fast, and auditable.

The architecture is designed for extensibility. New tools can be added via the `ToolsModule` trait. New observers can be added via the `ExecutionObserver` trait. The core engine is stable and well-tested.

Welcome to the team. You're building something important.
