"""Tests for Python-level entry points (python -m, find_ready_bin)."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

from ready_agent import find_ready_bin


class TestFindBin:
    def test_returns_existing_path(self):
        path = find_ready_bin()
        assert Path(path).is_file()

    def test_returns_string(self):
        path = find_ready_bin()
        assert isinstance(path, str)


class TestPythonModule:
    def test_python_m_help(self):
        result = subprocess.run(
            [sys.executable, "-m", "ready_agent", "--help"],
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert result.returncode == 0
        assert "Execute SOP-driven workflows" in result.stdout

    def test_python_m_matches_direct(self, run_ready):
        direct = run_ready("--help")
        module = subprocess.run(
            [sys.executable, "-m", "ready_agent", "--help"],
            capture_output=True,
            text=True,
            timeout=30,
        )
        assert direct.stdout == module.stdout
