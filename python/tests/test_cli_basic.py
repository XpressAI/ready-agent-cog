"""Basic CLI smoke tests: help, subcommands, error handling."""

from __future__ import annotations

import pytest


class TestHelp:
    def test_help_exits_zero(self, run_ready):
        result = run_ready("--help")
        assert result.returncode == 0

    def test_help_contains_description(self, run_ready):
        result = run_ready("--help")
        assert "Execute SOP-driven workflows" in result.stdout

    def test_help_lists_subcommands(self, run_ready):
        result = run_ready("--help")
        for cmd in ("plan", "run", "inspect", "tools"):
            assert cmd in result.stdout


class TestSubcommandHelp:
    @pytest.mark.parametrize("subcommand", ["plan", "run", "inspect", "tools"])
    def test_subcommand_help_exits_zero(self, run_ready, subcommand):
        result = run_ready(subcommand, "--help")
        assert result.returncode == 0

    @pytest.mark.parametrize("subcommand", ["plan", "run", "inspect", "tools"])
    def test_subcommand_help_contains_usage(self, run_ready, subcommand):
        result = run_ready(subcommand, "--help")
        assert "Usage:" in result.stdout


class TestErrorCases:
    def test_unknown_subcommand(self, run_ready):
        result = run_ready("foobar")
        assert result.returncode != 0

    def test_no_subcommand(self, run_ready):
        result = run_ready()
        assert result.returncode != 0
