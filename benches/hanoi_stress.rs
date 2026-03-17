//! Criterion benchmarks for the Towers of Hanoi stress test.
//!
//! This benchmark duplicates the test-local Hanoi game and tool infrastructure from
//! `tests/test_hanoi_stress.rs` so benchmark execution remains self-contained while
//! measuring parsing and interpreter execution separately.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use ready::Result;
use ready::execution::interpreter::PlanInterpreter;
use ready::execution::state::ExecutionState;
use ready::planning::parser::parse_python_to_plan;
use ready::tools::models::ToolCall;
use ready::tools::{
    InMemoryToolRegistry, ToolArgumentDescription, ToolDescription, ToolResult,
    ToolReturnDescription, ToolsModule,
};
use serde_json::{Map, Value, json};

/// Stateful Towers of Hanoi game — state container and primitives only.
///
/// The algorithm itself is expressed in the generated plan code.
/// This struct only provides the low-level operations that cannot be
/// expressed in the plan language: peg inspection and disc movement.
#[derive(Debug, Clone)]
struct HanoiGame {
    pegs: HashMap<String, Vec<i64>>,
    n_discs: i64,
    move_count: i64,
}

impl HanoiGame {
    const EMPTY_PEG: i64 = 999_999;

    fn new() -> Self {
        Self {
            pegs: HashMap::from([
                ("A".to_string(), Vec::new()),
                ("B".to_string(), Vec::new()),
                ("C".to_string(), Vec::new()),
            ]),
            n_discs: 0,
            move_count: 0,
        }
    }

    fn setup(&mut self, n_discs: i64) -> String {
        self.n_discs = n_discs;
        self.pegs = HashMap::from([
            ("A".to_string(), (1..=n_discs).rev().collect()),
            ("B".to_string(), Vec::new()),
            ("C".to_string(), Vec::new()),
        ]);
        self.move_count = 0;
        format!("Game setup with {n_discs} discs")
    }

    fn get_move_count(&self) -> i64 {
        self.move_count
    }

    fn get_disc_count(&self) -> i64 {
        self.n_discs
    }

    fn get_top_disc(&self, peg: &str) -> i64 {
        self.pegs
            .get(peg)
            .and_then(|stack| stack.last().copied())
            .unwrap_or(Self::EMPTY_PEG)
    }

    fn move_disc_between(&mut self, source: &str, dest: &str) -> String {
        let source_stack = self.pegs.get(source).expect("source peg should exist");
        assert!(
            !source_stack.is_empty(),
            "Cannot move from empty peg {source}"
        );

        let disc = *source_stack.last().expect("source peg should have a disc");
        if let Some(dest_top) = self.pegs.get(dest).and_then(|stack| stack.last()).copied() {
            assert!(
                dest_top >= disc,
                "Cannot place disc {disc} on smaller disc {dest_top}"
            );
        }

        self.pegs
            .get_mut(source)
            .expect("source peg should exist")
            .pop();
        self.pegs
            .get_mut(dest)
            .expect("destination peg should exist")
            .push(disc);
        self.move_count += 1;
        format!("Moved disc {disc}: {source} -> {dest}")
    }

    fn is_complete(&self) -> bool {
        self.pegs
            .get("C")
            .is_some_and(|stack| stack.len() as i64 == self.n_discs)
    }

    fn get_state(&self) -> Value {
        let mut pegs = Map::new();
        for peg in ["A", "B", "C"] {
            pegs.insert(
                peg.to_string(),
                Value::Array(
                    self.pegs
                        .get(peg)
                        .expect("peg should exist")
                        .iter()
                        .copied()
                        .map(Value::from)
                        .collect(),
                ),
            );
        }

        json!({
            "pegs": pegs,
            "move_count": self.move_count,
            "completed": self.is_complete(),
        })
    }
}

const HANOI_CODE: &str = r#"def main():
    setup_game({n_discs})
    done = is_game_complete()
    while not done:
        mc = get_move_count()
        nd = get_disc_count()
        parity = nd % 2
        step_idx = mc % 3
        if parity == 1 and step_idx == 0:
            peg1 = "A"
            peg2 = "C"
        elif parity == 1 and step_idx == 1:
            peg1 = "A"
            peg2 = "B"
        elif parity == 1 and step_idx == 2:
            peg1 = "B"
            peg2 = "C"
        elif parity == 0 and step_idx == 0:
            peg1 = "A"
            peg2 = "B"
        elif parity == 0 and step_idx == 1:
            peg1 = "A"
            peg2 = "C"
        else:
            peg1 = "B"
            peg2 = "C"
        top1 = get_top_disc(peg1)
        top2 = get_top_disc(peg2)
        if top1 < top2:
            move_disc_between(peg1, peg2)
        else:
            move_disc_between(peg2, peg1)
        done = is_game_complete()
    state = get_game_state()
"#;

fn hanoi_code(n_discs: i64) -> String {
    HANOI_CODE.replace("{n_discs}", &n_discs.to_string())
}

fn tool_desc(id: &str, args: &[&str], return_type: &str) -> ToolDescription {
    ToolDescription {
        id: id.to_string(),
        description: format!("Hanoi tool: {id}"),
        arguments: args
            .iter()
            .map(|name| ToolArgumentDescription {
                name: (*name).to_string(),
                description: String::new(),
                type_name: "Any".to_string(),
                default: None,
            })
            .collect(),
        returns: ToolReturnDescription {
            name: None,
            description: String::new(),
            type_name: Some(return_type.to_string()),
            fields: Vec::new(),
        },
    }
}

struct MockToolsModule {
    tools: Vec<ToolDescription>,
    handler: Arc<dyn Fn(&str, Vec<Value>) -> Result<ToolResult> + Send + Sync>,
}

impl ToolsModule for MockToolsModule {
    fn tools(&self) -> &[ToolDescription] {
        &self.tools
    }

    fn execute<'a>(
        &'a self,
        call: &'a ToolCall,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send + 'a>> {
        let result = (self.handler)(call.tool_id.as_str(), call.args.clone());
        Box::pin(async move { result })
    }
}

fn build_hanoi_registry(game: Arc<Mutex<HanoiGame>>) -> InMemoryToolRegistry {
    let tools = vec![
        tool_desc("setup_game", &["n_discs"], "str"),
        tool_desc("is_game_complete", &[], "bool"),
        tool_desc("get_move_count", &[], "int"),
        tool_desc("get_disc_count", &[], "int"),
        tool_desc("get_top_disc", &["peg"], "int"),
        tool_desc("move_disc_between", &["source", "dest"], "str"),
        tool_desc("get_game_state", &[], "dict"),
    ];

    let handler = Arc::new(
        move |tool_id: &str, args: Vec<Value>| -> Result<ToolResult> {
            let mut game = game.lock().expect("game mutex should not be poisoned");
            let value = match tool_id {
                "setup_game" => {
                    Value::String(game.setup(args[0].as_i64().expect("n_discs should be i64")))
                }
                "is_game_complete" => Value::Bool(game.is_complete()),
                "get_move_count" => Value::from(game.get_move_count()),
                "get_disc_count" => Value::from(game.get_disc_count()),
                "get_top_disc" => {
                    Value::from(game.get_top_disc(args[0].as_str().expect("peg should be str")))
                }
                "move_disc_between" => Value::String(game.move_disc_between(
                    args[0].as_str().expect("source should be str"),
                    args[1].as_str().expect("dest should be str"),
                )),
                "get_game_state" => game.get_state(),
                other => panic!("Unexpected tool id: {other}"),
            };

            Ok(ToolResult::Success(value))
        },
    );

    let mut registry = InMemoryToolRegistry::new();
    registry
        .register_module(Box::new(MockToolsModule { tools, handler }))
        .unwrap();
    registry
}

fn bench_hanoi_execution(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime should initialize");
    let mut group = c.benchmark_group("hanoi_execution");

    for &n_discs in &[3_i64, 10, 15] {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{n_discs}_discs")),
            &n_discs,
            |b, &n_discs| {
                let code = hanoi_code(n_discs);
                let plan = parse_python_to_plan(&code, "hanoi").expect("hanoi code should parse");

                b.iter(|| {
                    let game = Arc::new(Mutex::new(HanoiGame::new()));
                    let registry = Arc::new(build_hanoi_registry(game.clone()));
                    let interpreter = PlanInterpreter::new(registry, plan.clone())
                        .with_max_while_iterations(2_000_000);
                    let mut state = ExecutionState::default();

                    rt.block_on(interpreter.execute(&mut state))
                        .expect("execution should succeed");

                    let g = game.lock().expect("game mutex should not be poisoned");
                    assert_eq!(g.move_count, (1_i64 << n_discs) - 1);
                    assert!(g.is_complete());
                });
            },
        );
    }

    group.finish();
}

fn bench_hanoi_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("hanoi_parsing");

    for &n_discs in &[3_i64, 10, 20] {
        group.bench_with_input(
            BenchmarkId::from_parameter(format!("{n_discs}_discs")),
            &n_discs,
            |b, &n_discs| {
                let code = hanoi_code(n_discs);
                b.iter(|| parse_python_to_plan(&code, "hanoi").expect("hanoi code should parse"));
            },
        );
    }

    group.finish();
}

fn bench_hanoi_million_steps(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime should initialize");
    let mut group = c.benchmark_group("hanoi_million_steps");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(60));

    group.bench_function("20_discs_1M_moves", |b| {
        let code = hanoi_code(20);
        let plan = parse_python_to_plan(&code, "hanoi").expect("hanoi code should parse");

        b.iter(|| {
            let game = Arc::new(Mutex::new(HanoiGame::new()));
            let registry = Arc::new(build_hanoi_registry(game.clone()));
            let interpreter =
                PlanInterpreter::new(registry, plan.clone()).with_max_while_iterations(2_000_000);
            let mut state = ExecutionState::default();

            rt.block_on(interpreter.execute(&mut state))
                .expect("execution should succeed");

            let game = game.lock().expect("game mutex should not be poisoned");
            assert_eq!(game.move_count, 1_048_575);
            assert!(game.is_complete());
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_hanoi_parsing,
    bench_hanoi_execution,
    bench_hanoi_million_steps
);
criterion_main!(benches);
