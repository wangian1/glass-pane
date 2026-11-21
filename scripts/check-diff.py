#!/usr/bin/env python3
"""Check file selection, diff handoff, live refresh and return on a real terminal."""
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
    with tempfile.TemporaryDirectory(prefix="glasspane-diff-") as scratch:
        root = Path(scratch)
        repo = root / "repo"
        repo.mkdir()
        env = dict(os.environ, TERM="xterm-256color", XDG_CONFIG_HOME=str(root / "config"), XDG_STATE_HOME=str(root / "state"))
        env.pop("GLASSPANE_DEBUG_PANIC", None)

        def git(*args):
            return subprocess.check_output(["git", "-C", str(repo), *args], env=env, stderr=subprocess.STDOUT)

        git("init", "-b", "main")
        git("config", "user.name", "Test")
        git("config", "user.email", "test@example.invalid")
        git("config", "commit.gpgSign", "false")
        git("config", "core.hooksPath", str(root / "no-hooks"))
        file = repo / "file.txt"
        file.write_text("base\n")
        git("add", "file.txt")
        git("commit", "-m", "Original commit")
        file.write_text("INDEX_CONTENT\n")
        git("add", "file.txt")
        file.write_text("WORKTREE_VERSION\n")
        before = (repo / ".git/index").read_bytes()
        app = shell_checks.Dashboard(binary, repo, env)
        try:
            app.wait_for("Original commit")
            app.send("\t\t\t\r")
            app.wait_for("INDEX_CONTENT")
            app.escape()
            app.wait_for("Branch Graph")
            app.send("j\r")
            app.wait_for("WORKTREE_VERSION")
            file.write_text("zzzzzzzzzzzzzzzzzzzzzzzzzzzz\n")
            app.wait_for("zzzzzzzz", timeout=10)
            app.send("Ghlg")
            app.resize(40, 12)
            app.escape()
            app.send("\r")
            app.wait_for("zzzzzzzz")
            app.escape()
            app.finish()
            assert (repo / ".git/index").read_bytes() == before
            assert not (repo / ".git/index.lock").exists()
        finally:
            app.close()
        print("PASS: interactive diffs, live refresh, resize, return, and read-only index")


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/glasspane").resolve())
