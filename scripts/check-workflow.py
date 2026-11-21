#!/usr/bin/env python3
"""Exercise staging, reviewed commits and commit inspection on a controlling tty."""
import importlib.util
import os
from pathlib import Path
import select
import subprocess
import sys
import tempfile
import time

spec = importlib.util.spec_from_file_location("shell_checks", Path(__file__).with_name("check-shell.py"))
shell_checks = importlib.util.module_from_spec(spec)
spec.loader.exec_module(shell_checks)


def drain(app, seconds=0.3):
    # Let a queued batch of message keys finish before injecting SIGWINCH.
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if select.select([app.master], [], [], 0.02)[0]:
            app.buffer += os.read(app.master, 65536)


def wait_until(app, predicate, description):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        ready, _, _ = select.select([app.master], [], [], 0.05)
        if ready:
            app.buffer += os.read(app.master, 65536)
        if predicate():
            return
        if app.process.poll() is not None:
            break
    raise AssertionError(f"Timed out: {description}: {app.buffer[-3000:]!r}")


def run(binary, scenarios=None):
    for scenario in scenarios or ["flow", "cancel", "empty", "stale", "small", "hook_failure", "interrupt"]:
        with tempfile.TemporaryDirectory(prefix="glasspane-workflow-") as scratch:
            root = Path(scratch)
            repo = root / "repository with spaces"
            repo.mkdir()
            hooks = root / "hooks"
            hooks.mkdir()
            env = dict(os.environ, TERM="xterm-256color", XDG_CONFIG_HOME=str(root / "config"), XDG_STATE_HOME=str(root / "state"))
            env.pop("GLASSPANE_DEBUG_PANIC", None)

            def git(*args, check=True):
                result = subprocess.run(["git", "-C", str(repo), *args], env=env, capture_output=True, text=True)
                if check:
                    assert result.returncode == 0, result.stderr
                return result.stdout

            git("init", "-b", "main")
            for key, value in [("user.name", "Test"), ("user.email", "test@example.invalid"), ("commit.gpgSign", "false"), ("core.hooksPath", str(hooks))]:
                git("config", key, value)
            file = repo / "file.txt"
            file.write_text("COMMITTED_CONTENT\n")
            app = shell_checks.Dashboard(binary, repo, env)
            try:
                app.wait_for("UNTRACKED")
                app.send("\t\t\ts")
                wait_until(app, lambda: git("diff", "--cached", "--name-only").strip() == "file.txt", "stage selected file")
                # Await the worker completion and resulting dashboard snapshot.
                app.wait_for("STAGED (")
                if scenario == "flow":
                    app.send("u")
                    wait_until(app, lambda: not git("diff", "--cached", "--name-only").strip(), "unstage selected file")
                    app.wait_for("UNTRACKED")
                    app.send("s")
                    wait_until(app, lambda: git("diff", "--cached", "--name-only").strip() == "file.txt", "restage selected file")
                    app.wait_for("STAGED (")
                app.send("c")
                app.wait_for("Ctrl+S:")
                if scenario != "empty":
                    app.send("Workflow subject\r\rMessage body")
                if scenario == "cancel":
                    app.escape()
                elif scenario == "small":
                    drain(app)
                    app.resize(45, 16)
                    app.wait_for("Enlarge")
                    app.send("\x13")
                    drain(app)
                    app.resize(120, 36)
                    app.wait_for("Ctrl+S:")
                    app.escape()
                else:
                    if scenario == "stale":
                        file.write_text("changed after review\n")
                        git("add", "file.txt")
                    elif scenario in ["hook_failure", "interrupt"]:
                        hook = hooks / "pre-commit"
                        hook.write_text("#!/bin/sh\n" + ("printf 'COMMIT_HOOK_READY\\n'; sleep 30\n" if scenario == "interrupt" else "exit 1\n"))
                        hook.chmod(0o700)
                    elif scenario == "flow":
                        file.write_text("preserved unstaged edit\n")
                    app.send("\x13")
                    if scenario == "flow":
                        wait_until(app, lambda: git("log", "-1", "--format=%s", check=False).strip() == "Workflow subject", "commit from dialog")
                        assert git("show", "HEAD:file.txt") == "COMMITTED_CONTENT\n"
                        assert "Message body" in git("log", "-1", "--format=%B")
                        assert file.read_text() == "preserved unstaged edit\n"
                        app.wait_for("Committed")
                        app.send("\x1b[Z\r")  # Shift+Tab to Recent Commits, Enter inspect.
                        app.wait_for("Changed")
                        app.send("\r")
                        app.wait_for("COMMITTED_CONTENT")
                        app.escape()
                        app.wait_for("Changed")
                        app.escape()
                    else:
                        if scenario == "interrupt":
                            app.wait_for("COMMIT_HOOK_READY")
                            app.send("\x03")
                        app.wait_for("Enter" if scenario == "empty" else "changed" if scenario == "stale" else "unchanged")
                        assert not git("rev-parse", "--verify", "HEAD", check=False).strip()
                        if scenario == "hook_failure":
                            (hooks / "pre-commit").unlink()
                            app.send("\x13")
                            wait_until(app, lambda: git("log", "-1", "--format=%s", check=False).strip() == "Workflow subject", "retry preserves message")
                            app.wait_for("Committed")
                        else:
                            app.escape()
                if scenario not in ["flow", "hook_failure"]:
                    assert not git("rev-parse", "--verify", "HEAD", check=False).strip()
                assert not (repo / ".git/index.lock").exists()
                if scenario != "hook_failure":
                    app.wait_for("Staging")
                app.finish()
            finally:
                app.close()
            print(f"PASS: interactive workflow {scenario}")


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/glass").resolve(), sys.argv[2:])
