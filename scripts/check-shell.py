#!/usr/bin/env python3
"""Exercise shell handoff on a real pseudo-terminal using disposable repositories.

Usage: python3 scripts/check-shell.py target/debug/glasspane
Python is needed only for this integration check, not to run glasspane.
"""
import fcntl
import os
from pathlib import Path
import select
import shlex
import struct
import subprocess
import sys
import tempfile
import termios
import time


class Dashboard:
    def __init__(self, binary, repo, env, *, follow=False, explicit_path=True):
        self.master, self.slave = os.openpty()
        self.resize(120, 36)
        self.original = termios.tcgetattr(self.master)
        self.buffer = b""

        def controlling_terminal():
            os.setsid()
            fcntl.ioctl(0, termios.TIOCSCTTY, 0)

        self.process = subprocess.Popen(
            [str(binary)] + ([] if follow else ["--no-follow"]) + ([str(repo)] if explicit_path else []),
            cwd=repo,
            stdin=self.slave, stdout=self.slave, stderr=self.slave,
            env=env, preexec_fn=controlling_terminal,
        )

    def resize(self, width, height):
        fcntl.ioctl(self.slave, termios.TIOCSWINSZ, struct.pack("HHHH", height, width, 0, 0))

    def wait_for(self, text, timeout=8):
        text = text.encode()
        deadline = time.monotonic() + timeout
        while text not in self.buffer:
            if time.monotonic() >= deadline:
                raise AssertionError(f"Timed out waiting for {text!r}: {self.buffer[-4000:]!r}")
            ready, _, _ = select.select([self.master], [], [], 0.1)
            if ready:
                self.buffer += os.read(self.master, 65536)
            elif self.process.poll() is not None:
                raise AssertionError(f"Dashboard exited before {text!r}: {self.buffer[-4000:]!r}")
        end = self.buffer.index(text) + len(text)
        result, self.buffer = self.buffer[:end], self.buffer[end:]
        return result

    def wait_for_branch(self, repo, expected):
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            ready, _, _ = select.select([self.master], [], [], 0.05)
            if ready: self.buffer += os.read(self.master, 65536)
            result = subprocess.run(["git", "-C", str(repo), "symbolic-ref", "--short", "HEAD"], capture_output=True, text=True)
            if result.returncode == 0 and result.stdout.strip() == expected: return
        raise AssertionError(f"branch did not switch to {expected}: {self.buffer[-2000:]!r}")

    def send(self, text):
        os.write(self.master, text.encode())

    def escape(self):
        self.send("\x1b")
        # A standalone Escape also prefixes Alt-key encodings. Let the terminal
        # parser resolve it before another key, while draining redraw output.
        deadline = time.monotonic() + 0.15
        while time.monotonic() < deadline:
            ready, _, _ = select.select([self.master], [], [], 0.02)
            if ready: self.buffer += os.read(self.master, 65536)

    def finish(self):
        self.send("q")
        # Keep draining the pty while quitting: a full output buffer can block a
        # redraw before the dashboard reaches its next keyboard poll.
        deadline = time.monotonic() + 8
        while self.process.poll() is None and time.monotonic() < deadline:
            ready, _, _ = select.select([self.master], [], [], 0.1)
            if ready:
                self.buffer += os.read(self.master, 65536)
        assert self.process.poll() == 0, "dashboard did not quit successfully"
        # Consume any trailing final card written immediately before process exit.
        while select.select([self.master], [], [], 0)[0]:
            chunk = os.read(self.master, 65536)
            if not chunk: break
            self.buffer += chunk
        assert "╭─ glasspane".encode() in self.buffer and (b"Staging" in self.buffer or b"Staged" in self.buffer), "quit did not leave a compact summary"
        assert termios.tcgetattr(self.master) == self.original, "terminal attributes did not restore"

    def close(self):
        if self.process.poll() is None:
            # Only terminate the dashboard created by this test; closing the pty
            # also hangs up any remaining foreground child.
            self.process.kill()
            self.process.wait(timeout=5)
        os.close(self.master)
        os.close(self.slave)


def run(binary):
    with tempfile.TemporaryDirectory(prefix="glasspane-shell-") as scratch:
        root = Path(scratch)
        repo = root / "repository with spaces"
        repo.mkdir()
        for args in [("init", "-b", "main"), ("config", "user.name", "Shell Test"),
                     ("config", "user.email", "shell@example.invalid")]:
            subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True)
        rc = root / "zsh"
        rc.mkdir()
        (rc / ".zshenv").write_text("export GLASSPANE_TEST_ENV_LOADED=1\n")
        (rc / ".zshrc").write_text("PROMPT='SHELL_READY> '\nalias glasspane_test_alias='printf ALIAS_READY'\n")
        editor = root / "test-editor.sh"
        marker = root / "editor-ran"
        editor.write_text("#!/bin/sh\nset -eu\ntest -t 0\n"
                          "printf 'Shell editor commit\\n' > \"$1\"\n"
                          f"printf 'ok' > {shlex.quote(str(marker))}\n")
        editor.chmod(0o700)
        env = dict(os.environ, SHELL="/bin/zsh", HOME=str(root), ZDOTDIR=str(rc),
                   XDG_CONFIG_HOME=str(root / "config"), XDG_STATE_HOME=str(root / "state"),
                   GIT_EDITOR=shlex.quote(str(editor)), TERM="xterm-256color")
        env.pop("GLASSPANE_DEBUG_PANIC", None)
        app = Dashboard(binary, repo, env)
        try:
            app.wait_for("glasspane")
            app.send("!")
            initial = app.wait_for("SHELL_READY>")
            assert b"No commits yet" in initial
            assert b"0 staged" in initial
            app.send("printf 'CONFIG_%s\\n' \"$GLASSPANE_TEST_ENV_LOADED\"; glasspane_test_alias; printf '\\n'; printf '%s\\n' \"$ZDOTDIR\"\r")
            app.wait_for("CONFIG_1")
            app.wait_for("ALIAS_READY")
            setup = app.wait_for("SHELL_READY>")
            assert str(rc).encode() in setup, "original ZDOTDIR was not restored"
            app.send("printf 'hello\\n' > 'file with spaces.txt'; git add -- 'file with spaces.txt'\r")
            staged = app.wait_for("SHELL_READY>")
            assert b"1 staged" in staged
            app.send("git commit\r")
            app.wait_for("Shell editor commit")
            committed = app.wait_for("SHELL_READY>")
            assert b"Shell editor commit" in committed, "prompt card did not refresh the latest commit"
            assert b"0 staged" in committed
            assert marker.read_text() == "ok", "Git editor was not given a terminal"
            app.send("false\r")
            app.wait_for("SHELL_READY>")
            app.send("printf 'STATUS_%s\\n' \"$?\"\r")
            app.wait_for("STATUS_1")
            app.wait_for("SHELL_READY>")
            app.send("git checkout -b summary-test\r")
            branch = app.wait_for("SHELL_READY>")
            assert b"Branch  summary-test" in branch, "prompt card did not follow branch checkout"
            app.send("cd ..\r")
            outside = app.wait_for("SHELL_READY>")
            assert b"Not a git repository" in outside
            app.send("cd 'repository with spaces'\r")
            returned = app.wait_for("SHELL_READY>")
            assert b"Branch  summary-test" in returned
            app.send("exit\r")
            app.wait_for("\x1b[?1049h")
            app.wait_for("Shell editor commit")
            message = subprocess.check_output(["git", "-C", str(repo), "log", "-1", "--format=%s"], text=True)
            assert message.strip() == "Shell editor commit"
            app.send("!")
            app.wait_for("SHELL_READY>")
            shell_group = os.tcgetpgrp(app.master)
            app.send("printf 'RUNNING_SLEEP\\n'; sleep 20\r")
            app.wait_for("RUNNING_SLEEP\r\n")
            deadline = time.monotonic() + 5
            while os.tcgetpgrp(app.master) == shell_group and time.monotonic() < deadline:
                time.sleep(0.02)
            assert os.tcgetpgrp(app.master) != shell_group, "shell did not start the foreground job"
            app.send("\x03")
            app.wait_for("SHELL_READY>")
            assert app.process.poll() is None, "Ctrl+C in the shell killed the dashboard"
            app.resize(80, 24)
            app.send("\x04")
            app.wait_for("\x1b[?1049h")
            app.wait_for("glasspane")
            app.finish()
        finally:
            app.close()
        print("PASS: shell summary refresh, startup config, exit status, editor, interrupts, EOF, resize, final card")

        app = Dashboard(binary, repo, dict(env, SHELL=str(root / "missing-shell")))
        try:
            app.wait_for("glasspane")
            app.send("!")
            app.wait_for("Cannot")
            app.wait_for("missing-shell")
            app.finish()
        finally:
            app.close()
        print("PASS: failed shell spawn returns to dashboard and restores terminal")

        app = Dashboard(binary, repo, env)
        try:
            app.wait_for("glasspane")
            app.send("!")
            app.wait_for("SHELL_READY>")
            app.send("exit 7\r")
            app.wait_for("\x1b[?1049h")
            app.wait_for("glasspane")
            app.finish()
        finally:
            app.close()
        print("PASS: nonzero shell exit returns to dashboard")

        # An ordinary zsh with no ZDOTDIR must still load HOME startup files,
        # and should see ZDOTDIR unset rather than our temporary proxy path.
        (root / ".zshenv").write_text((rc / ".zshenv").read_text())
        (root / ".zshrc").write_text((rc / ".zshrc").read_text())
        plain_env = dict(env)
        plain_env.pop("ZDOTDIR", None)
        app = Dashboard(binary, repo, plain_env)
        try:
            app.wait_for("glasspane")
            app.send("!")
            initial = app.wait_for("SHELL_READY>")
            assert b"Branch  summary-test" in initial
            app.send("printf 'ZDOTDIR_SET_%s\\n' \"${+ZDOTDIR}\"\r")
            app.wait_for("ZDOTDIR_SET_0")
            app.wait_for("SHELL_READY>")
            app.send("exit\r")
            app.wait_for("\x1b[?1049h")
            app.wait_for("glasspane")
            app.finish()
        finally:
            app.close()
        print("PASS: HOME startup configuration works with ZDOTDIR unset")

        other = root / "other repository"
        other.mkdir()
        subprocess.run(["git", "-C", str(other), "init", "-b", "main"], check=True, capture_output=True)
        subprocess.run(["git", "-C", str(other), "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "--allow-empty", "-m", "Other"], check=True, capture_output=True)
        subprocess.run(["git", "-C", str(other), "checkout", "-b", "protected"], check=True, capture_output=True)
        other_index = (other / ".git/index").read_bytes()
        action_env = dict(env, GIT_DIR=str(other / ".git"), GIT_WORK_TREE=str(other), GIT_INDEX_FILE=str(other / ".git/index"))
        app = Dashboard(binary, repo, action_env)
        try:
            app.wait_for("summary-test")
            app.send("\tj\r")
            app.wait_for_branch(repo, "main")
            app.finish()
            assert b"Branch  main" in app.buffer, "final summary did not show the branch selected with Enter"
            assert subprocess.check_output(["git", "-C", str(other), "symbolic-ref", "--short", "HEAD"], text=True).strip() == "protected"
            assert (other / ".git/index").read_bytes() == other_index
        finally:
            app.close()
        print("PASS: keyboard checkout targets the displayed repository despite Git environment overrides; final summary reflects checkout")


if __name__ == "__main__":
    run(Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/glasspane").resolve())
