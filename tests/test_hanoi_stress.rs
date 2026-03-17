//! Towers of Hanoi stress test for the WhileStep implementation.
//!
//! Proves the Eddy Agent System can handle 1M+ steps via a Towers of Hanoi
//! game expressed as a while loop. The game logic is implemented as test-local
//! tools; the plan is parsed from intermediate Python code.
//!
//! The **algorithm itself** is expressed in the plan code (`HANOI_CODE`)
//! using the classic iterative peg-pair cycling method:
//!
//! * The parity check (`nd % 2`) and step index (`mc % 3`) are expressed
//!   directly in the plan as semantic binary expressions parsed from Python
//!   modulo operations — no tool call needed.
//! * The peg-pair selection is a 6-branch `if/elif/else` — each branch
//!   becomes a `SwitchStep` evaluated by the interpreter.
//! * The move-direction decision is an `if/else` comparison — another
//!   `SwitchStep`.
//! * Variable assignments (`peg1 = "A"`) become `AssignStep` nodes.
//!
//! This ensures the algorithmic work (control flow, branching, arithmetic,
//! comparison) happens *within* the Eddy step execution, not hidden inside
//! tool code.
//!
//! Disc counts and expected move counts:
//!
//! ```text
//! ===  ============
//!  N   Moves (2^N-1)
//! ===  ============
//!  3            7
//! 10        1,023
//! 15       32,767
//! 20    1,048,575
//! ===  ============
//! ```

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ready::Result;
use ready::execution::interpreter::PlanInterpreter;
use ready::execution::state::{ExecutionState, ExecutionStatus};
use ready::plan::{AbstractPlan, BinaryOperator, ConditionalBranch, Expression, Step};
use ready::planning::parser::parse_python_to_plan;
use ready::tools::models::ToolCall;
use ready::tools::{
    InMemoryToolRegistry, ToolArgumentDescription, ToolDescription, ToolResult,
    ToolReturnDescription, ToolsModule,
};
use serde_json::{Map, Value, json};

/// Stateful Towers of Hanoi game — state container and primitives only.
///
/// The algorithm itself is expressed in the plan code (`HANOI_CODE`).
/// This struct only provides the low-level operations that cannot be
/// expressed in the plan language: peg inspection and disc movement.
/// Arithmetic (`%`) is a built-in plan operation — no tool needed.
#[derive(Debug, Clone)]
struct HanoiGame {
    pegs: HashMap<String, Vec<i64>>,
    n_discs: i64,
    move_count: i64,
}

impl HanoiGame {
    // Sentinel value for an empty peg (larger than any disc number).
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

    /// Initialize the game with `n_discs` on peg A.
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

    // Return the current move count.
    fn get_move_count(&self) -> i64 {
        self.move_count
    }

    // Return the total number of discs.
    fn get_disc_count(&self) -> i64 {
        self.n_discs
    }

    // Return the top disc number on `peg`, or `EMPTY_PEG` if empty.
    fn get_top_disc(&self, peg: &str) -> i64 {
        self.pegs
            .get(peg)
            .and_then(|stack| stack.last().copied())
            .unwrap_or(Self::EMPTY_PEG)
    }

    // Move the top disc from `source` to `dest`. Validates legality.
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

    // Check if all discs are on peg C.
    fn is_complete(&self) -> bool {
        self.pegs
            .get("C")
            .map_or(false, |stack| stack.len() as i64 == self.n_discs)
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
                        .map(|disc| Value::from(disc))
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

fn parse_hanoi_plan(n_discs: i64) -> AbstractPlan {
    parse_python_to_plan(&hanoi_code(n_discs), "main").expect("hanoi code should parse")
}

fn expect_tool_step<'a>(step: &'a Step, expected_tool_id: &str) -> &'a Option<String> {
    match step {
        Step::ToolStep {
            tool_id,
            output_variable,
            ..
        } => {
            assert_eq!(tool_id, expected_tool_id);
            output_variable
        }
        other => panic!("Expected ToolStep for {expected_tool_id}, got {other:?}"),
    }
}

fn expect_assign_step<'a>(step: &'a Step, expected_variable: &str) -> &'a Expression {
    match step {
        Step::AssignStep { target, value } => {
            assert_eq!(target, expected_variable);
            value
        }
        other => panic!("Expected AssignStep for {expected_variable}, got {other:?}"),
    }
}

fn expect_switch_step(step: &Step) -> &Vec<ConditionalBranch> {
    match step {
        Step::SwitchStep { branches } => branches,
        other => panic!("Expected SwitchStep, got {other:?}"),
    }
}

fn expect_while_step(step: &Step) -> (&Expression, &Vec<Step>) {
    match step {
        Step::WhileStep { condition, body } => (condition, body),
        other => panic!("Expected WhileStep, got {other:?}"),
    }
}

async fn run_hanoi(n_discs: i64, max_iterations: Option<usize>) -> (Value, Duration) {
    let game = Arc::new(Mutex::new(HanoiGame::new()));
    let registry = Arc::new(build_hanoi_registry(game.clone()));
    let plan = parse_hanoi_plan(n_discs);

    let interpreter = match max_iterations {
        Some(max) => PlanInterpreter::new(registry, plan.clone()).with_max_while_iterations(max),
        None => PlanInterpreter::new(registry, plan.clone()),
    };

    let mut state = ExecutionState::default();
    let start = Instant::now();
    interpreter
        .execute(&mut state)
        .await
        .expect("execution should succeed");
    let duration = start.elapsed();

    assert_eq!(state.status, ExecutionStatus::Completed);
    assert!(
        game.lock()
            .expect("game mutex should not be poisoned")
            .is_complete(),
        "Game not complete — not all discs on peg C"
    );

    let game_state = game
        .lock()
        .expect("game mutex should not be poisoned")
        .get_state();
    (game_state, duration)
}

mod test_hanoi_parsing {
    use super::*;

    #[test]
    fn test_hanoi_code_parses_to_correct_top_level_structure() {
        let plan = parse_hanoi_plan(3);

        assert_eq!(plan.steps.len(), 4);
        assert_eq!(expect_tool_step(&plan.steps[0], "setup_game"), &None);
        assert_eq!(
            expect_tool_step(&plan.steps[1], "is_game_complete"),
            &Some("done".to_string())
        );
        let _ = expect_while_step(&plan.steps[2]);
        assert_eq!(
            expect_tool_step(&plan.steps[3], "get_game_state"),
            &Some("state".to_string())
        );
    }

    #[test]
    fn test_while_body_contains_algorithm_steps() {
        // The while body expresses the full algorithm with Eddy steps.
        let plan = parse_hanoi_plan(3);
        let (_, body) = expect_while_step(&plan.steps[2]);

        assert_eq!(body.len(), 9);
        assert_eq!(
            expect_tool_step(&body[0], "get_move_count"),
            &Some("mc".to_string())
        );
        assert_eq!(
            expect_tool_step(&body[1], "get_disc_count"),
            &Some("nd".to_string())
        );

        let parity = expect_assign_step(&body[2], "parity");
        match parity {
            Expression::BinaryExpression { operator, .. } => {
                assert_eq!(operator, &BinaryOperator::Modulo)
            }
            other => panic!("Expected BinaryExpression, got {other:?}"),
        }

        let step_idx = expect_assign_step(&body[3], "step_idx");
        match step_idx {
            Expression::BinaryExpression { operator, .. } => {
                assert_eq!(operator, &BinaryOperator::Modulo)
            }
            other => panic!("Expected BinaryExpression, got {other:?}"),
        }

        assert_eq!(expect_switch_step(&body[4]).len(), 6);
        assert_eq!(
            expect_tool_step(&body[5], "get_top_disc"),
            &Some("top1".to_string())
        );
        assert_eq!(
            expect_tool_step(&body[6], "get_top_disc"),
            &Some("top2".to_string())
        );
        assert_eq!(expect_switch_step(&body[7]).len(), 2);
        assert_eq!(
            expect_tool_step(&body[8], "is_game_complete"),
            &Some("done".to_string())
        );
    }

    #[test]
    fn test_peg_pair_branches_contain_assignments() {
        // Each peg-pair branch sets peg1 and peg2 via semantic assignment.
        let plan = parse_hanoi_plan(3);
        let (_, body) = expect_while_step(&plan.steps[2]);
        let branches = expect_switch_step(&body[4]);

        assert_eq!(branches.len(), 6);
        for branch in branches {
            assert_eq!(branch.steps.len(), 2);
            let _ = expect_assign_step(&branch.steps[0], "peg1");
            let _ = expect_assign_step(&branch.steps[1], "peg2");
        }
    }

    #[test]
    fn test_move_direction_branches_contain_tool_call() {
        // Each direction branch calls move_disc_between.
        let plan = parse_hanoi_plan(3);
        let (_, body) = expect_while_step(&plan.steps[2]);
        let branches = expect_switch_step(&body[7]);

        assert_eq!(branches.len(), 2);
        for branch in branches {
            assert_eq!(branch.steps.len(), 1);
            let _ = expect_tool_step(&branch.steps[0], "move_disc_between");
        }
    }
}

mod test_hanoi_execution {
    use super::*;

    #[tokio::test]
    async fn test_hanoi_3_discs() {
        // 3 discs = 7 moves. Quick sanity check.
        let (game_state, elapsed) = run_hanoi(3, None).await;
        assert_eq!(game_state["move_count"], json!(7));
        println!(
            "\n  Hanoi 3 discs: {} moves in {:.4?}",
            game_state["move_count"], elapsed
        );
    }

    #[tokio::test]
    async fn test_hanoi_10_discs() {
        // 10 discs = 1,023 moves.
        let (game_state, elapsed) = run_hanoi(10, Some(2_000)).await;
        assert_eq!(game_state["move_count"], json!(1_023));
        println!(
            "\n  Hanoi 10 discs: {} moves in {:.4?}",
            game_state["move_count"], elapsed
        );
    }

    #[tokio::test]
    //#[ignore]
    async fn test_hanoi_15_discs() {
        // 15 discs = 32,767 moves.
        let (game_state, elapsed) = run_hanoi(15, Some(35_000)).await;
        assert_eq!(game_state["move_count"], json!(32_767));
        println!(
            "\n  Hanoi 15 discs: {} moves in {:.4?}",
            game_state["move_count"], elapsed
        );
    }

    #[tokio::test]
    #[ignore]
    async fn test_hanoi_20_discs() {
        // 20 discs = 1,048,575 moves. THE MILLION-STEP TEST.
        let (game_state, elapsed) = run_hanoi(20, Some(2_000_000)).await;
        assert_eq!(game_state["move_count"], json!(1_048_575));
        println!(
            "\n  Hanoi 20 discs: {} moves in {:.2?}",
            game_state["move_count"], elapsed
        );
    }
}
