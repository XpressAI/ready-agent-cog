"""Tests for the ``ready tools`` subcommand."""

from __future__ import annotations

from pathlib import Path


class TestToolsListing:
    def test_exits_zero_with_shell_tools(self, run_ready, tmp_shell_tools):
        result = run_ready("tools", "--tools", str(tmp_shell_tools))
        assert result.returncode == 0

    def test_lists_shell_tool(self, run_ready, tmp_shell_tools):
        result = run_ready("tools", "--tools", str(tmp_shell_tools))
        assert "echo_message" in result.stdout
        assert "Echoes a message back" in result.stdout

    def test_lists_builtin_tools(self, run_ready, tmp_shell_tools):
        result = run_ready("tools", "--tools", str(tmp_shell_tools))
        assert "delegate_to_large_language_model" in result.stdout
        assert "extract_from_plaintext" in result.stdout
        assert "sort" in result.stdout


class TestToolsErrors:
    def test_nonexistent_tools_file_still_lists_builtins(self, run_ready):
        """A missing shell-tools file is not fatal — builtins are still shown."""
        result = run_ready("tools", "--tools", "/nonexistent/shell-tools.json")
        assert result.returncode == 0
        assert "delegate_to_large_language_model" in result.stdout
