#!/usr/bin/env python3
"""Startup must use the calling cwd even when another shell left saved tracking state."""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile

spec = importlib.util.spec_from_file_location("shell_checks", Path(__file__).with_name("check-shell.py"))
shell_checks = importlib.util.module_from_spec(spec)
spec.loader.exec_module(shell_checks)


def run(binary):
    for explicit_path in [False, True]:
        with tempfile.TemporaryDirectory(prefix="glasspane-startup-") as scratch:
            root = Path(scratch).resolve()
            repo = root / "launch repo"
            other = root / "other repo"
            repo.mkdir()
            other.mkdir()
            rc = root / "zsh"
            rc.mkdir()
            (rc / ".zshrc").write_text("PROMPT='STARTUP_SHELL> '\n")
            env = dict(os.environ, TERM="xterm-256color", SHELL="/bin/zsh", ZDOTDIR=str(rc),
                       XDG_CONFIG_HOME=str(root / "config"), XDG_STATE_HOME=str(root / "state"))
            env.pop("GLASSPANE_DEBUG_PANIC", None)
            for path, branch in [(repo, "launch-branch"), (other, "followed-branch")]:
                subprocess.run(["git", "init", "-b", branch, str(path)], env=env, check=True, capture_output=True)
            state = root / "state/glasspane/cwd"
            state.parent.mkdir(parents=True)
            state.write_text("/\n")
            app = shell_checks.Dashboard(binary, repo, env, follow=True, explicit_path=explicit_path)
            try:
                app.wait_for("launch-branch")
                app.send("!")
                app.wait_for("STARTUP_SHELL>")
                app.send("printf 'LAUNCH_PWD=%s\\n' \"$PWD\"\r")
                app.wait_for(f"LAUNCH_PWD={repo}")
                app.send("exit\r")
                app.wait_for("launch-branch")
                # Saved root was just a baseline, but subsequent changes still follow.
                state.write_text(f"{other}\n")
                app.wait_for("followed-branch", timeout=10)
                app.finish()
            finally:
                app.close()
            print(f"PASS: {binary.name} {'explicit path' if explicit_path else 'calling directory'} ignores saved root; shell cwd and later following work")


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/glass").resolve())
