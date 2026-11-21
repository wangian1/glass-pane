#!/usr/bin/env python3
"""Exercise amend review, editors and cancellation on a controlling pseudo-terminal."""
import importlib.util
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile

spec = importlib.util.spec_from_file_location("shell_checks", Path(__file__).with_name("check-shell.py"))
shell_checks = importlib.util.module_from_spec(spec)
spec.loader.exec_module(shell_checks)
Dashboard = shell_checks.Dashboard


def run(binary):
    for mode in [1, 2, 3, "cancel", "interrupt", "stale", "small"]:
        with tempfile.TemporaryDirectory(prefix="glasspane-amend-") as scratch:
            root = Path(scratch)
            repo = root / "repository with spaces"
            repo.mkdir()
            hooks = root / "hooks"
            hooks.mkdir()
            rc = root / "zsh"
            rc.mkdir()
            (rc / ".zshrc").write_text("PROMPT='READY> '\n")
            env = dict(os.environ, HOME=str(root), ZDOTDIR=str(rc), SHELL="/bin/zsh", TERM="xterm-256color",
                       XDG_CONFIG_HOME=str(root / "config"), XDG_STATE_HOME=str(root / "state"))
            env.pop("GLASSPANE_DEBUG_PANIC", None)
            env.pop("NO_COLOR", None)

            def git(*args):
                return subprocess.check_output(["git", "-C", str(repo), *args], env=env, stderr=subprocess.STDOUT, text=True)

            git("init", "-b", "main")
            for key, value in [("user.name", "Test"), ("user.email", "test@example.invalid"), ("core.hooksPath", str(hooks)), ("commit.gpgSign", "false")]:
                git("config", key, value)
            file = repo / "file.txt"
            file.write_text("base\n")
            git("add", "file.txt")
            git("commit", "-m", "Original subject")
            old = git("rev-parse", "HEAD").strip()
            original_tree = git("rev-parse", "HEAD^{tree}").strip()
            file.write_text("staged change\n")
            git("add", "file.txt")
            file.write_text("staged change\nunstaged change\n")
            index_before = (repo / ".git/index").read_bytes()
            editor = root / "test editor.sh"
            marker = root / "editor-used"
            if mode == "interrupt":
                body = "printf 'AMEND_EDITOR_READY\\n'; sleep 30"
            elif mode == 1:
                body = "exit 99"
            else:
                body = "printf 'Edited subject\\n\\nBody\\n' > \"$1\""
            editor.write_text(f"#!/bin/sh\ntest -t 0 || exit 20\nprintf ok > {shlex.quote(str(marker))}\n{body}\n")
            editor.chmod(0o700)
            env["GIT_EDITOR"] = shlex.quote(str(editor))
            app = Dashboard(binary, repo, env)
            try:
                app.wait_for("Original subject")
                if mode == "small": app.resize(45, 16)
                app.send("a")
                if mode == "small":
                    app.wait_for("Enlarge")
                    app.send("\r")
                    app.escape()
                else:
                    app.wait_for("Amend latest commit")
                    app.wait_for("Enter: confirm amend")
                    if mode == "cancel":
                        app.escape()
                    else:
                        if mode == "stale":
                            git("commit", "-m", "External commit")
                            external = git("rev-parse", "HEAD").strip()
                        choice = mode if isinstance(mode, int) else 2
                        app.send(f"{choice}\r")
                        if mode == "interrupt":
                            app.wait_for("AMEND_EDITOR_READY")
                            app.send("\x03")
                            app.wait_for("unchanged")
                            assert app.process.poll() is None
                        elif mode == "stale":
                            app.wait_for("changed")
                            assert git("rev-parse", "HEAD").strip() == external
                        else:
                            app.wait_for("Amended")
                app.finish()
                new = git("rev-parse", "HEAD").strip()
                if mode in ["cancel", "interrupt", "small"]:
                    assert new == old
                    assert (repo / ".git/index").read_bytes() == index_before
                elif mode != "stale":
                    assert new != old
                    subject = git("log", "-1", "--format=%s").strip()
                    assert subject == ("Original subject" if mode == 1 else "Edited subject")
                    if mode == 2:
                        assert git("rev-parse", "HEAD^{tree}").strip() == original_tree
                        assert git("diff", "--cached", "--name-only").strip() == "file.txt"
                    else:
                        assert git("show", "HEAD:file.txt") == "staged change\n"
                        assert git("diff", "--cached", "--name-only").strip() == ""
                    assert marker.exists() == (mode != 1)
                assert file.read_text() == "staged change\nunstaged change\n"
                assert not (repo / ".git/index.lock").exists()
            finally:
                app.close()
            print(f"PASS: interactive amend {mode}")


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/glass").resolve())
