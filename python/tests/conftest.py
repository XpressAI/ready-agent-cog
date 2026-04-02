"""Shared test fixtures for ready-agent CLI tests."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path
from typing import Any

import pytest

from ready_agent import find_ready_bin


@pytest.fixture(scope="session")
def ready_bin() -> str:
    """Return the path to the ``ready`` binary."""
    return find_ready_bin()


@pytest.fixture(scope="session")
def run_ready(ready_bin: str):
    """Return a helper that invokes the ``ready`` binary and captures output."""

    def _run(*args: str, input: str | None = None) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [ready_bin, *args],
            capture_output=True,
            text=True,
            input=input,
            timeout=30,
        )

    return _run


# ---------------------------------------------------------------------------
# Fixture: a minimal plan with only AssignSteps (no tools needed)
# ---------------------------------------------------------------------------

ASSIGN_ONLY_PLAN: dict[str, Any] = {
    "name": "assign_test",
    "description": "A plan with only assignment steps.",
    "steps": [
        {
            "type": "AssignStep",
            "target": "x",
            "value": {"type": "Literal", "value": 42},
        },
        {
            "type": "AssignStep",
            "target": "greeting",
            "value": {"type": "Literal", "value": "hello world"},
        },
    ],
    "code": "def main():\n    x = 42\n    greeting = 'hello world'\n",
}


@pytest.fixture()
def tmp_plan_file(tmp_path: Path) -> Path:
    """Write ``ASSIGN_ONLY_PLAN`` to a temporary JSON file."""
    p = tmp_path / "test_plan.json"
    p.write_text(json.dumps(ASSIGN_ONLY_PLAN))
    return p


# ---------------------------------------------------------------------------
# Fixture: plan with a UserInteractionStep (has prefillable inputs)
# ---------------------------------------------------------------------------

PLAN_WITH_INPUT: dict[str, Any] = {
    "name": "input_test",
    "description": "A plan that asks for user input.",
    "steps": [
        {
            "type": "UserInteractionStep",
            "prompt": "Enter your name",
            "output_variable": "user_name",
        },
        {
            "type": "AssignStep",
            "target": "done",
            "value": {"type": "Literal", "value": True},
        },
    ],
    "code": "def main():\n    user_name = input('Enter your name')\n    done = True\n",
}


@pytest.fixture()
def tmp_plan_with_input(tmp_path: Path) -> Path:
    """Write ``PLAN_WITH_INPUT`` to a temporary JSON file."""
    p = tmp_path / "input_plan.json"
    p.write_text(json.dumps(PLAN_WITH_INPUT))
    return p


# ---------------------------------------------------------------------------
# Fixture: a plan with a ToolStep (needs tools registered)
# ---------------------------------------------------------------------------

PLAN_WITH_TOOL: dict[str, Any] = {
    "name": "tool_test",
    "description": "A plan that calls a tool.",
    "steps": [
        {
            "type": "ToolStep",
            "tool_id": "echo_message",
            "arguments": [{"type": "Literal", "value": "hello"}],
            "output_variable": "result",
        },
    ],
    "code": "def main():\n    result = echo_message('hello')\n",
}


@pytest.fixture()
def tmp_plan_with_tool(tmp_path: Path) -> Path:
    """Write ``PLAN_WITH_TOOL`` to a temporary JSON file."""
    p = tmp_path / "tool_plan.json"
    p.write_text(json.dumps(PLAN_WITH_TOOL))
    return p


# ---------------------------------------------------------------------------
# Fixture: shell-tools JSON file
# ---------------------------------------------------------------------------

SHELL_TOOLS: dict[str, Any] = {
    "echo_message": {
        "description": "Echoes a message back.",
        "template": ["echo", "{message}"],
        "arguments": [
            {
                "name": "message",
                "description": "The message to echo.",
                "type_name": "str",
            }
        ],
        "returns": {
            "name": None,
            "description": "The echoed message.",
            "type_name": "str",
            "fields": [],
        },
        "active": True,
        "output_parsing": "raw",
        "output_schema": None,
    }
}


@pytest.fixture()
def tmp_shell_tools(tmp_path: Path) -> Path:
    """Write a sample shell-tools JSON file."""
    p = tmp_path / "shell-tools.json"
    p.write_text(json.dumps(SHELL_TOOLS))
    return p


# ---------------------------------------------------------------------------
# Fixture: SOP text file
# ---------------------------------------------------------------------------

@pytest.fixture()
def tmp_sop_file(tmp_path: Path) -> Path:
    """Write a simple SOP text file."""
    p = tmp_path / "test_sop.txt"
    p.write_text("Step 1: Echo hello world using the echo_message tool.\n")
    return p
