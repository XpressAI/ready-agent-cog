"""Tests for the ``ready run`` subcommand."""

from __future__ import annotations

from pathlib import Path


class TestRunErrors:
    def test_missing_plan_and_sop(self, run_ready):
        """Must provide either --plan or --sop."""
        result = run_ready("run")
        assert result.returncode != 0
        combined = result.stdout + result.stderr
        assert "Either --sop or --plan must be provided" in combined

    def test_nonexistent_plan_file(self, run_ready):
        result = run_ready("run", "--plan", "/nonexistent/plan.json")
        assert result.returncode != 0

    def test_invalid_input_format(self, run_ready, tmp_plan_file, tmp_shell_tools):
        """--input without '=' should fail."""
        result = run_ready(
            "run",
            "--plan", str(tmp_plan_file),
            "--tools", str(tmp_shell_tools),
            "--input", "badformat",
        )
        assert result.returncode != 0
        combined = result.stdout + result.stderr
        assert "Expected NAME=VALUE" in combined

    def test_unknown_prefillable_input(self, run_ready, tmp_plan_file, tmp_shell_tools):
        """--input for a variable not in the plan should fail."""
        result = run_ready(
            "run",
            "--plan", str(tmp_plan_file),
            "--tools", str(tmp_shell_tools),
            "--input", "unknown_var=value",
        )
        assert result.returncode != 0
        combined = result.stdout + result.stderr
        assert "Unknown prefillable input" in combined


class TestRunExecution:
    def test_assign_only_plan_completes(self, run_ready, tmp_plan_file, tmp_shell_tools):
        """A plan with only AssignSteps (no tool calls) should complete successfully."""
        result = run_ready(
            "run",
            "--plan", str(tmp_plan_file),
            "--tools", str(tmp_shell_tools),
        )
        assert result.returncode == 0
        assert "Completed" in result.stdout

    def test_plan_with_shell_tool(self, run_ready, tmp_plan_with_tool, tmp_shell_tools):
        """A plan calling a registered shell tool should complete."""
        result = run_ready(
            "run",
            "--plan", str(tmp_plan_with_tool),
            "--tools", str(tmp_shell_tools),
        )
        assert result.returncode == 0
        assert "Completed" in result.stdout

    def test_plan_with_prefilled_input(self, run_ready, tmp_plan_with_input, tmp_shell_tools):
        """A plan with UserInteractionStep should complete when input is pre-filled."""
        result = run_ready(
            "run",
            "--plan", str(tmp_plan_with_input),
            "--tools", str(tmp_shell_tools),
            "--input", "user_name=Alice",
        )
        assert result.returncode == 0
        assert "Completed" in result.stdout
