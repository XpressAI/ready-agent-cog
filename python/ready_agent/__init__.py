"""Ready Agent -- A plan-based agent execution engine."""

from __future__ import annotations

import os
import subprocess
import sys
import sysconfig
from pathlib import Path


def find_ready_bin() -> str:
    """Find the ``ready`` binary bundled with this package.

    Searches the standard script/bin directories where pip, uv, and other
    installers place console-script executables.
    """
    exe = "ready.exe" if sys.platform == "win32" else "ready"

    # 1. scripts dir (standard pip / uv install)
    scripts = sysconfig.get_path("scripts")
    if scripts:
        path = Path(scripts) / exe
        if path.is_file():
            return str(path)

    # 2. Same directory as the running Python interpreter
    path = Path(sys.executable).parent / exe
    if path.is_file():
        return str(path)

    # 3. User-scheme scripts (pip install --user)
    try:
        user_scripts = sysconfig.get_path("scripts", scheme="posix_user")
    except KeyError:
        user_scripts = None
    if user_scripts:
        path = Path(user_scripts) / exe
        if path.is_file():
            return str(path)

    # 4. PATH fallback
    from shutil import which

    found = which("ready")
    if found:
        return found

    raise FileNotFoundError(
        f"Could not find the '{exe}' binary. "
        "Make sure ready-agent is installed correctly: pip install ready-agent"
    )


def main() -> None:
    """Entry point that forwards all arguments to the ``ready`` binary."""
    try:
        ready = find_ready_bin()
    except FileNotFoundError as exc:
        print(str(exc), file=sys.stderr)
        sys.exit(1)

    if sys.platform == "win32":
        result = subprocess.run([ready, *sys.argv[1:]])
        sys.exit(result.returncode)
    else:
        os.execvp(ready, [ready, *sys.argv[1:]])
