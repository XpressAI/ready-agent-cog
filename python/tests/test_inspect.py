"""Tests for the ``ready inspect`` subcommand."""

from __future__ import annotations

from pathlib import Path


class TestInspectPlan:
    def test_exits_zero(self, run_ready, tmp_plan_file):
        result = run_ready("inspect", "--plan", str(tmp_plan_file))
        assert result.returncode == 0

    def test_shows_plan_name(self, run_ready, tmp_plan_file):
        result = run_ready("inspect", "--plan", str(tmp_plan_file))
        assert "assign_test" in result.stdout

    def test_shows_steps(self, run_ready, tmp_plan_file):
        result = run_ready("inspect", "--plan", str(tmp_plan_file))
        # The formatted plan should show variable assignments
        assert "x" in result.stdout
        assert "greeting" in result.stdout

    def test_shows_prefillable_inputs_section(self, run_ready, tmp_plan_file):
        result = run_ready("inspect", "--plan", str(tmp_plan_file))
        assert "Prefillable Inputs" in result.stdout

    def test_no_prefillable_inputs_shows_none(self, run_ready, tmp_plan_file):
        result = run_ready("inspect", "--plan", str(tmp_plan_file))
        assert "(none)" in result.stdout


class TestInspectWithInputs:
    def test_shows_input_variable(self, run_ready, tmp_plan_with_input):
        result = run_ready("inspect", "--plan", str(tmp_plan_with_input))
        assert result.returncode == 0
        assert "user_name" in result.stdout

    def test_shows_input_hint(self, run_ready, tmp_plan_with_input):
        result = run_ready("inspect", "--plan", str(tmp_plan_with_input))
        assert "--input" in result.stdout


class TestInspectErrors:
    def test_nonexistent_file(self, run_ready):
        result = run_ready("inspect", "--plan", "/nonexistent/path.json")
        assert result.returncode != 0

    def test_invalid_json(self, run_ready, tmp_path):
        bad_file = tmp_path / "bad.json"
        bad_file.write_text("not json at all")
        result = run_ready("inspect", "--plan", str(bad_file))
        assert result.returncode != 0

    def test_missing_plan_arg(self, run_ready):
        result = run_ready("inspect")
        assert result.returncode != 0
