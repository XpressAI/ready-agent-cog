use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Parser, Subcommand};
use serde_json::Value;
use tracing::{debug, error};
use tracing_subscriber::EnvFilter;

use ready::execution::observer::LoggingObserver;
use ready::llm::client::OpenAiClient;
use ready::llm::traits::LlmClient;
use ready::plan::AbstractPlan;
use ready::plan_format::format_plan;
use ready::tools::process::load_plans_from_directory;
use ready::tools::{
    BuiltinToolsModule, InMemoryToolRegistry, ProcessToolsModule, ShellToolStore, ShellToolsModule,
    ToolsModule,
};
use ready::workflow::{SopExecutor, SopPlanner};
use ready::{ReadyError, Result};

const DEFAULT_SHELL_TOOLS_PATH: &str = "shell-tools.json";

#[derive(Parser)]
#[command(
    name = "ready",
    about = "Ready Agent System",
    long_about = "Execute SOP-driven workflows by generating plans with an LLM and running them against pluggable tool registries, including builtins, shell tools, and reusable process tools."
)]
struct Cli {
    #[arg(long, global = true, help = "Enable verbose debug output for plan loading and execution.")]
    debug: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate an execution plan from an SOP document using an LLM.
    Plan {
        #[arg(
            long,
            help = "Path to the SOP (Standard Operating Procedure) document."
        )]
        sop: String,
        #[arg(
            long,
            help = "Path to a shell-tools JSON file (default: shell-tools.json)."
        )]
        tools: Option<String>,
        #[arg(
            long,
            help = "Directory containing reusable sub-plans to register as process tools."
        )]
        plans_dir: Option<String>,
        #[arg(long, help = "LLM model name to use for planning (e.g. gpt-4o).")]
        model: Option<String>,
        #[arg(
            long,
            help = "Output file path for the generated plan JSON (default: derived from SOP filename)."
        )]
        output: Option<String>,
    },
    /// Execute a plan, either from a pre-generated plan file or by planning on-the-fly from an SOP.
    Run {
        #[arg(long, help = "Path to an SOP document to plan and execute on-the-fly.")]
        sop: Option<String>,
        #[arg(
            long,
            help = "Path to a pre-generated plan JSON file to execute directly."
        )]
        plan: Option<String>,
        #[arg(
            long,
            help = "Path to a shell-tools JSON file (default: shell-tools.json)."
        )]
        tools: Option<String>,
        #[arg(
            long,
            help = "Directory containing reusable sub-plans to register as process tools."
        )]
        plans_dir: Option<String>,
        #[arg(long, help = "LLM model name to use when planning from an SOP.")]
        model: Option<String>,
        #[arg(
            long = "input",
            value_name = "NAME=VALUE",
            help = "Pre-fill a plan input variable. May be repeated. VALUE accepts JSON, or plain text if JSON parsing fails."
        )]
        inputs: Vec<String>,
    },
    /// Display a human-readable summary of a previously generated plan, including prefillable inputs.
    Inspect {
        #[arg(long, help = "Path to the plan JSON file to inspect.")]
        plan: String,
    },
    /// List all available tools (builtins, shell tools, and process tools).
    Tools {
        #[arg(
            long,
            help = "Path to a shell-tools JSON file (default: shell-tools.json)."
        )]
        tools: Option<String>,
        #[arg(
            long,
            help = "Directory containing reusable sub-plans to register as process tools."
        )]
        plans_dir: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    init_tracing(Cli::parse().debug);

    if let Err(error) = run().await {
        error!(error = %error, "command failed");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    let debug = cli.debug;
    match cli.command {
        Commands::Plan {
            sop,
            tools,
            plans_dir,
            model,
            output,
        } => {
            handle_plan(
                &sop,
                tools.as_deref(),
                plans_dir.as_deref(),
                model,
                output.as_deref(),
                debug,
            )
            .await
        }
        Commands::Run {
            sop,
            plan,
            tools,
            plans_dir,
            model,
            inputs,
        } => {
            handle_run(
                sop.as_deref(),
                plan.as_deref(),
                tools.as_deref(),
                plans_dir.as_deref(),
                model,
                &inputs,
                debug,
            )
            .await
        }
        Commands::Inspect { plan } => handle_inspect(&plan, debug),
        Commands::Tools { tools, plans_dir } => {
            handle_tools(tools.as_deref(), plans_dir.as_deref(), debug)
        }
    }
}

fn init_tracing(debug_enabled: bool) {
    let filter = if debug_enabled {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("ready=trace,ready_agent_cog=trace"))
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"))
    };

    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_writer(std::io::stderr)
        .try_init();
}

async fn handle_plan(
    sop_path: &str,
    tools_path: Option<&str>,
    plans_dir: Option<&str>,
    model: Option<String>,
    output_path: Option<&str>,
    debug: bool,
) -> Result<()> {
    let sop_path = PathBuf::from(sop_path);
    debug_log(debug, format!("Planning SOP from {}", sop_path.display()));
    let llm = Arc::new(OpenAiClient::new(model, None, None));
    let registry = Arc::new(build_registry(llm.clone(), tools_path, plans_dir)?);
    let plan = generate_plan(llm, registry.as_ref(), &sop_path).await?;
    debug_log(debug, format!("Generated plan:\n{}", format_plan(&plan)));

    let output_path = output_path
        .map(PathBuf::from)
        .unwrap_or_else(|| default_plan_output_path(&sop_path));
    fs::write(&output_path, serde_json::to_string_pretty(&plan)?)?;
    println!("Saved plan to {}", output_path.display());
    Ok(())
}

async fn handle_run(
    sop_path: Option<&str>,
    plan_path: Option<&str>,
    tools_path: Option<&str>,
    plans_dir: Option<&str>,
    model: Option<String>,
    inputs: &[String],
    debug: bool,
) -> Result<()> {
    let llm = Arc::new(OpenAiClient::new(model, None, None));
    let registry = Arc::new(build_registry(llm.clone(), tools_path, plans_dir)?);

    let plan = match (plan_path, sop_path) {
        (Some(plan_path), _) => {
            debug_log(debug, format!("Loading plan from {plan_path}"));
            load_plan(Path::new(plan_path))?
        }
        (None, Some(sop_path)) => {
            let sop_path = PathBuf::from(sop_path);
            debug_log(debug, format!("Generating plan from SOP {}", sop_path.display()));
            generate_plan(llm, registry.as_ref(), &sop_path).await?
        }
        (None, None) => {
            return Err(ReadyError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Either --sop or --plan must be provided",
            )));
        }
    };

    let initial_inputs = parse_prefilled_inputs(inputs)?;
    validate_prefilled_inputs(&plan, &initial_inputs)?;

    if debug {
        debug!(plan = %format_plan(&plan), "execution plan loaded");
        debug!(
            initial_inputs = %serde_json::to_string_pretty(&initial_inputs)?,
            "initial inputs parsed"
        );
    }

    let executor = SopExecutor::new(registry, Some(Arc::new(LoggingObserver)));
    let state = executor
        .execute(&plan, initial_inputs, Some(Box::new(prompt_for_input)))
        .await?;

    if debug {
        debug!(
            interpreter_state = %serde_json::to_string_pretty(&state.interpreter_state)?,
            "final interpreter state"
        );
    }

    println!("Execution finished with status: {:?}", state.status);
    if let Some(error) = state.error {
        return Err(ReadyError::Execution {
            step_index: error.step_index,
            step_type: error.step_type,
            message: error.message,
        });
    }

    Ok(())
}

fn handle_inspect(plan_path: &str, debug: bool) -> Result<()> {
    debug_log(debug, format!("Inspecting plan from {plan_path}"));
    let plan = load_plan(Path::new(plan_path))?;
    print!("{}", format_plan(&plan));
    print_prefillable_inputs(&plan);
    Ok(())
}

fn handle_tools(tools_path: Option<&str>, plans_dir: Option<&str>, debug: bool) -> Result<()> {
    let llm = Arc::new(OpenAiClient::default());
    let registry = build_registry(llm, tools_path, plans_dir)?;
    debug_log(debug, format!("Loaded {} tools", registry.tools().len()));
    for tool in registry.tools() {
        println!("{} - {}", tool.id, tool.description);
    }
    Ok(())
}

fn build_registry(
    llm: Arc<dyn LlmClient>,
    tools_path: Option<&str>,
    plans_dir: Option<&str>,
) -> Result<InMemoryToolRegistry> {
    let mut registry = InMemoryToolRegistry::new();
    registry.register_module(Box::new(BuiltinToolsModule::new(llm)))?;

    let shell_tools_path = tools_path.unwrap_or(DEFAULT_SHELL_TOOLS_PATH);
    let shell_tools = ShellToolStore::load(shell_tools_path)?;
    if !shell_tools.is_empty() {
        registry.register_module(Box::new(ShellToolsModule::new(shell_tools)))?;
    }

    if let Some(plans_dir) = plans_dir {
        let plans = load_plans_from_directory(plans_dir)?;
        let process_module = ProcessToolsModule::new(plans, registry.clone())?;
        if !process_module.tools().is_empty() {
            registry.register_module(Box::new(process_module))?;
        }
    }

    Ok(registry)
}

fn debug_log(enabled: bool, message: impl AsRef<str>) {
    if enabled {
        debug!(message = %message.as_ref());
    }
}

async fn generate_plan(
    llm: Arc<dyn LlmClient>,
    registry: &InMemoryToolRegistry,
    sop_path: &Path,
) -> Result<AbstractPlan> {
    let sop_text = fs::read_to_string(sop_path)?;
    let planner = SopPlanner::new(llm, 3);
    let mut plan = planner.plan(&sop_text, registry.tools().as_slice()).await?;
    plan.name = default_plan_name(sop_path);
    Ok(plan)
}

fn default_plan_name(sop_path: &Path) -> String {
    sop_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("plan")
        .to_string()
}

fn default_plan_output_path(sop_path: &Path) -> PathBuf {
    let stem = sop_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("plan");
    sop_path.with_file_name(format!("{stem}_plan.json"))
}

fn load_plan(path: &Path) -> Result<AbstractPlan> {
    let content = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&content)?)
}

fn parse_prefilled_inputs(inputs: &[String]) -> Result<HashMap<String, Value>> {
    let mut parsed = HashMap::new();

    for input in inputs {
        let (name, raw_value) = input.split_once('=').ok_or_else(|| {
            ReadyError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Invalid --input '{input}'. Expected NAME=VALUE."),
            ))
        })?;

        let name = name.trim();
        if name.is_empty() {
            return Err(ReadyError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Invalid --input '{input}'. Input name cannot be empty."),
            )));
        }

        let value = parse_input_value(raw_value);
        parsed.insert(name.to_string(), value);
    }

    Ok(parsed)
}

fn validate_prefilled_inputs(plan: &AbstractPlan, cli_inputs: &HashMap<String, Value>) -> Result<()> {
    let prefillable_inputs = plan.prefillable_inputs();
    let available_inputs = prefillable_inputs
        .iter()
        .map(|input| input.variable_name.as_str())
        .collect::<Vec<_>>();

    for input_name in cli_inputs.keys() {
        if !available_inputs.iter().any(|name| name == input_name) {
            let known_inputs = if available_inputs.is_empty() {
                "none".to_string()
            } else {
                available_inputs.join(", ")
            };
            return Err(ReadyError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "Unknown prefillable input '{input_name}'. Available prefillable inputs: {known_inputs}."
                ),
            )));
        }
    }

    Ok(())
}

fn parse_input_value(raw_value: &str) -> Value {
    serde_json::from_str(raw_value).unwrap_or_else(|_| Value::String(raw_value.to_string()))
}

fn print_prefillable_inputs(plan: &AbstractPlan) {
    println!("\n--- Prefillable Inputs ---");
    for line in prefillable_input_lines(plan) {
        println!("{line}");
    }
}

fn prefillable_input_lines(plan: &AbstractPlan) -> Vec<String> {
    let inputs = plan.prefillable_inputs();
    if inputs.is_empty() {
        return vec!["  (none)".to_string()];
    }

    inputs
        .into_iter()
        .map(|input| format!("  --input {}=<value>  # {}", input.variable_name, input.prompt))
        .collect()
}

fn prompt_for_input(prompt: &str) -> Option<String> {
    print!("{prompt}: ");
    let _ = io::stdout().flush();

    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(_) => Some(input.trim().to_string()),
        Err(_) => None,
    }
}
