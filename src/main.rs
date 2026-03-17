use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::{Parser, Subcommand};

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
#[command(name = "ready", about = "Ready Agent System")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Plan {
        #[arg(long)]
        sop: String,
        #[arg(long)]
        tools: Option<String>,
        #[arg(long)]
        plans_dir: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        output: Option<String>,
    },
    Run {
        #[arg(long)]
        sop: Option<String>,
        #[arg(long)]
        plan: Option<String>,
        #[arg(long)]
        tools: Option<String>,
        #[arg(long)]
        plans_dir: Option<String>,
        #[arg(long)]
        model: Option<String>,
    },
    Inspect {
        #[arg(long)]
        plan: String,
    },
    Tools {
        #[arg(long)]
        tools: Option<String>,
        #[arg(long)]
        plans_dir: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
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
            )
            .await
        }
        Commands::Run {
            sop,
            plan,
            tools,
            plans_dir,
            model,
        } => {
            handle_run(
                sop.as_deref(),
                plan.as_deref(),
                tools.as_deref(),
                plans_dir.as_deref(),
                model,
            )
            .await
        }
        Commands::Inspect { plan } => handle_inspect(&plan),
        Commands::Tools { tools, plans_dir } => {
            handle_tools(tools.as_deref(), plans_dir.as_deref())
        }
    }
}

async fn handle_plan(
    sop_path: &str,
    tools_path: Option<&str>,
    plans_dir: Option<&str>,
    model: Option<String>,
    output_path: Option<&str>,
) -> Result<()> {
    let sop_path = PathBuf::from(sop_path);
    let llm = Arc::new(OpenAiClient::new(model, None, None));
    let registry = Arc::new(build_registry(llm.clone(), tools_path, plans_dir)?);
    let plan = generate_plan(llm, registry.as_ref(), &sop_path).await?;

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
) -> Result<()> {
    let llm = Arc::new(OpenAiClient::new(model, None, None));
    let registry = Arc::new(build_registry(llm.clone(), tools_path, plans_dir)?);

    let plan = match (plan_path, sop_path) {
        (Some(plan_path), _) => load_plan(Path::new(plan_path))?,
        (None, Some(sop_path)) => {
            let sop_path = PathBuf::from(sop_path);
            generate_plan(llm, registry.as_ref(), &sop_path).await?
        }
        (None, None) => {
            return Err(ReadyError::Io(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Either --sop or --plan must be provided",
            )));
        }
    };

    let executor = SopExecutor::new(registry, Some(Arc::new(LoggingObserver)));
    let state = executor
        .execute(&plan, HashMap::new(), Some(Box::new(prompt_for_input)))
        .await?;

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

fn handle_inspect(plan_path: &str) -> Result<()> {
    let plan = load_plan(Path::new(plan_path))?;
    print!("{}", format_plan(&plan));
    Ok(())
}

fn handle_tools(tools_path: Option<&str>, plans_dir: Option<&str>) -> Result<()> {
    let llm = Arc::new(OpenAiClient::default());
    let registry = build_registry(llm, tools_path, plans_dir)?;
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

fn prompt_for_input(prompt: &str) -> Option<String> {
    print!("{prompt}: ");
    let _ = io::stdout().flush();

    let mut input = String::new();
    match io::stdin().read_line(&mut input) {
        Ok(_) => Some(input.trim().to_string()),
        Err(_) => None,
    }
}
