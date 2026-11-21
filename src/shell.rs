//! Explicit user-controlled shell handoff; snapshot loaders remain read-only.
use anyhow::{Context, Result};
use crossterm::{
    cursor::Show,
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::{
    io::{self, Write},
    path::Path,
    process::{Command, Stdio},
};

/// Return a recoverable message if the shell cannot start. Terminal handoff or
/// restoration errors are fatal, so the caller's terminal guard can clean up.
pub fn open(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    cwd: &Path,
) -> Result<Option<String>> {
    with_terminal(terminal, || -> Result<Option<String>> {
        writeln!(
            io::stdout(),
            "glasspane shell — run commands here; exit or Ctrl+D returns to the dashboard."
        )?;
        io::stdout().flush()?;

        let shell = std::env::var_os("SHELL")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                if cfg!(target_os = "macos") {
                    "/bin/zsh"
                } else {
                    "/bin/sh"
                }
                .into()
            });
        let executable = std::env::current_exe()?;
        let startup = if crate::shell_startup::is_zsh(Path::new(&shell)) {
            Some(crate::shell_startup::zsh_startup()?)
        } else {
            None
        };
        let mut command = Command::new(&shell);
        if let Some(startup) = &startup {
            let original = std::env::var_os("ZDOTDIR");
            command
                .env("GLASSPANE_BIN", &executable)
                .env(
                    "GLASSPANE_ZDOTDIR_SET",
                    if original.is_some() { "1" } else { "0" },
                )
                .env("GLASSPANE_USER_ZDOTDIR", original.unwrap_or_default())
                .env("ZDOTDIR", startup.path());
        } else {
            // Other shells receive an initial summary without changing their startup.
            let _ = Command::new(&executable)
                .arg("summary")
                .current_dir(cwd)
                .status();
        }
        // No command string is constructed. Commands are entered by the user in an
        // ordinary interactive shell, including editor prompts and authentication.
        reset_child_interrupt(&mut command);
        let outcome = command
            .arg("-i")
            .current_dir(cwd)
            .env("GLASSPANE_SHELL", "1")
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status();
        drop(startup);

        Ok(outcome.err().map(|e| {
            format!(
                "Cannot open shell {} in {}: {e}",
                Path::new(&shell).display(),
                cwd.display()
            )
        }))
    })?
}

/// Temporarily return the tty to an interactive command, then force a full redraw.
pub fn with_terminal<T>(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    operation: impl FnOnce() -> T,
) -> Result<T> {
    let _interrupt = ParentInterrupt::ignore()?;
    disable_raw_mode().context("Cannot release terminal raw mode")?;
    execute!(io::stdout(), LeaveAlternateScreen, Show)?;
    let outcome = operation();
    // Restore even after a failed spawn or nonzero exit. Resizing and clearing
    // invalidate Ratatui's old diff buffer after an editor or shell used the tty.
    enable_raw_mode().context("Cannot restore dashboard raw mode")?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    // A fullscreen resize also invalidates the diff buffer when size is unchanged.
    // Unlike clear(), it does not query a shell/editor's old cursor position.
    terminal.resize(terminal.size()?.into())?;
    Ok(outcome)
}

// The parent waits without consuming input. Ctrl+C must reach the editor/Git or
// shell, not terminate the dashboard while its terminal is temporarily released.
struct ParentInterrupt(libc::sigaction);
impl ParentInterrupt {
    fn ignore() -> io::Result<Self> {
        // SAFETY: initialize a POSIX sigaction, install SIG_IGN, and retain the
        // previous disposition for restoration. This handler executes no Rust.
        unsafe {
            let mut previous: libc::sigaction = std::mem::zeroed();
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = libc::SIG_IGN;
            libc::sigemptyset(&mut action.sa_mask);
            if libc::sigaction(libc::SIGINT, &action, &mut previous) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Self(previous))
        }
    }
}
impl Drop for ParentInterrupt {
    fn drop(&mut self) {
        // SAFETY: restore the disposition saved by sigaction for this handoff.
        unsafe {
            libc::sigaction(libc::SIGINT, &self.0, std::ptr::null_mut());
        }
    }
}

pub(crate) fn reset_child_interrupt(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    // SAFETY: the post-fork closure uses only signal syscalls and errno access;
    // it does not allocate, lock, or touch shared application state.
    unsafe {
        command.pre_exec(|| {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = libc::SIG_DFL;
            libc::sigemptyset(&mut action.sa_mask);
            if libc::sigaction(libc::SIGINT, &action, std::ptr::null_mut()) != 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
}
