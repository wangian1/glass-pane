# glasspane

A live Git dashboard with file staging, reviewed commits, commit inspection, safe branch switching, latest-commit amending, and an optional interactive shell in the same terminal. It displays your branch, HEAD, recent commits, a branch graph, local and remote-tracking branches, and grouped file status. Edits, staging, commits, checkouts and manual fetches trigger background refreshes.

**Validation:** the release build, all 81 Rust tests, the interactive shell integration checks, Clippy with warnings denied, and formatting pass. Native FSEvents cannot start in the sandbox used for testing; live refresh was verified through its polling fallback. See [VALIDATION.md](VALIDATION.md) for remaining manual acceptance work.

## Install and launch

Install the current stable Rust toolchain and Apple's Command Line Tools (`xcode-select --install`) on macOS. Git must be on `PATH` for the graph; the other panels use libgit2, bundled by the Rust bindings. SSH/HTTPS support in the bundled libgit2 is disabled; commands you enter in shell mode use your normal Git executable.

```sh
cargo install --path . --force
glass                       # everyday shortcut
glasspane                    # full command; same application
glasspane ~/code/project     # discover from a specific directory
glasspane --no-follow        # ignore shell directory tracking
glasspane --commits 100
glasspane --print-config
glasspane summary            # print the compact dashboard without opening the TUI
```

Both executables are installed by Cargo and accept the same options and subcommands. No shell alias is needed. `cargo run` starts `glasspane`; `cargo run --bin glass` starts the shortcut.

If upgrading from gitpane, replace `eval "$(gitpane init zsh)"` in `~/.zshrc` with `eval "$(glass init zsh)"`, then reload the shell. Existing `~/.config/gitpane/config.toml` settings are used as a fallback until you create `~/.config/glasspane/config.toml` (the same applies under `$XDG_CONFIG_HOME`). No configuration files are moved or overwritten.

For development:

```sh
cargo run -- /path/to/repository
./scripts/check.sh
python3 scripts/check-shell.py target/release/glasspane
python3 scripts/check-amend.py target/release/glasspane
```

The dashboard runs outside a repository too: it displays a message, and can switch to a repository when shell tracking reports a new directory. Press `r` to retry discovery manually.

## Run Git commands from the dashboard

Press **`!`** to temporarily leave the dashboard and open your `$SHELL` in the directory shown in the header. You get a normal interactive shell, including your aliases, command history, and Git commit editor.

For example, once the shell prompt appears:

```sh
git add path/to/file
git commit -m "Describe the change"
exit
```

A compact card above the zsh prompt shows the **branch**, **latest commit**, and counts of **staged, unstaged, untracked and conflicted files**. It refreshes before each prompt, including after commits, staging, checkouts and directory changes. This is inline terminal output: it does not overwrite command output or update while you are typing. Other shells receive an initial card when opened.

Use `git commit` without `-m` to open your configured editor. **`exit` or Ctrl+D** returns to the dashboard and triggers a full refresh. Ctrl+C cancels a shell command rather than quitting the dashboard. The dashboard is paused while the shell owns the terminal.

This is a child shell: changing directory there changes the dashboard's directory only when the optional cwd hook below is active. Shell commands run with your normal permissions and can modify the repository or contact remotes when you explicitly request it. Background data loading remains read-only and never fetches automatically; the Branches panel can perform an explicit safe checkout when you press Enter.

Quitting the full-screen TUI also leaves one fresh compact card in your normal terminal. Run `glasspane summary` (optionally followed by a path) to print it directly. Cards adapt to terminal width and respect `NO_COLOR`.

The zsh prompt hook is installed only in the child shell through temporary startup files. Your existing `.zshenv`, `.zshrc`, aliases, prompt and configured `ZDOTDIR` are preserved; no shell configuration is edited. Prompt summaries do not change the previous command's exit status. The summary only reads HEAD and status, avoiding graph and branch-history loading.

If `$SHELL` is unset, macOS uses `/bin/zsh` and other Unix systems use `/bin/sh`. A failed shell launch returns to the dashboard with a message in help.

## Shell directory tracking (optional)

Add this to `~/.zshrc` after installing glasspane:

```zsh
eval "$(glasspane init zsh)"
```

The printed snippet registers a `chpwd` hook, creates `${XDG_STATE_HOME:-$HOME/.local/state}/glasspane`, and writes the shell's directory to its `cwd` file. The dashboard watches the file's parent, so replacement of the file is supported. Switching into another repository switches the dashboard. If the state directory does not exist yet, the dashboard detects its creation within two seconds.

Without the hook, the header marks the selected launch directory as `(launch dir)`. `--no-follow` disables tracking. Startup always uses the current directory, or an explicit `PATH` when provided. Saved tracking state is treated as a baseline, so a previous shell's directory cannot override the launch directory. Later changes to the tracking file are followed.

**Multiple shells use last-writer-wins behavior.** There is one state file, not one per terminal. Paths containing spaces are supported. Removing the state file returns the dashboard to its launch directory.

The application does not modify your shell configuration automatically.

## Navigation

| Key | Action |
|---|---|
| `Tab` / `Shift+Tab` | Next / previous panel |
| `j` / `k`, `↓` / `↑` | Select a branch, recent commit, or staging file; scroll graph |
| `g` / `G` | First / last branch, commit, file, or graph bounds |
| `Enter` | Switch branch; inspect recent commit; open staging diff; expand graph |
| `Space` | Expand / restore any focused panel |
| `s` / `u` | Stage / unstage the selected file in Staging |
| `c` | Review staged files, write a message, and commit |
| `r` | Full refresh |
| `a` | Review and amend the latest commit |
| `!` | Open a shell; `exit` / Ctrl+D returns and refreshes |
| `?` / `Esc` | Toggle / close help; Esc also restores an expanded panel |
| `q` / `Ctrl+C` | Quit |

At widths of at least 100 columns and sufficient height, all four panels appear in a grid. Smaller terminals show the focused panel; use `Tab` to switch. Press **Space** to expand any focused panel to full width and height; press Space again or Esc to restore the grid. Enter also expands Graph. The minimum usable terminal is 30×12. Scroll positions are retained across refreshes and clamped when content shrinks.

The graph uses Unicode connectors and Git-assigned lane colors, aligned hash/message columns, and separate branch-label badges. Current HEAD is bold with a green commit dot and label. Long messages and labels are truncated at Unicode grapheme boundaries; expanding the panel shows more text.

`NO_COLOR` disables application colors. Operation states such as merges, rebases, cherry-picks and bisects appear in the header. A partially staged file appears in both STAGED and UNSTAGED. Conflicts appear in their own group.

## Stage and unstage files

Focus **Staging**, use **↑/↓** or **j/k** to select a file, then press **s** to stage the whole file or **u** on a STAGED entry to unstage it. Files, deletions, and detected renames are supported. The action updates only the selected paths, then refreshes the dashboard. **u preserves all working-tree edits**, including new files before the first commit. Unstaging a rename resets both its old/new index paths without moving files on disk.

For a partially staged file, **s on its UNSTAGED entry includes all remaining edits**, while **u on its STAGED entry removes its staged changes**. This is whole-file staging; individual hunk staging is not implemented. Conflicted entries require explicit resolution/staging in shell mode. Git index locks are respected; conflicting actions cannot run simultaneously.

## Create a commit

Press **c** to open a review showing the branch, included staged files, and counts of unstaged/untracked entries excluded. Type your message in the dialog. **Enter** adds a line; arrows, Home/End, Backspace and Delete edit the message. **PgUp/PgDn** scroll the staged-file list. **Ctrl+S** commits; **Esc** or **Ctrl+C** cancels the dialog. A minimum **60×20** terminal is required to confirm.

Only the reviewed index is committed. HEAD, the branch and staged contents are checked again immediately before Git runs; a changed review is refused. Initial commits are supported. Empty messages, unresolved conflicts and active merge/rebase/other operations are refused. No files are staged automatically. Failed commits retain the message for retry; stale reviews must be cancelled and reopened.

Native `git commit` runs with the normal terminal so hooks and signing prompts retain interactive I/O. Git's configured hooks and signing are honored. Ctrl+C during a hook returns to the dialog. The dashboard refreshes afterward; a successful commit closes the dialog. No push is performed.

## Inspect a commit

Focus **Recent Commits**, select a row with **↑/↓** or **j/k**, then press **Enter**. The full-screen inspection shows the full commit ID, author, date, message and changed files. **Tab** switches between scrolling the message and selecting files. **PgUp/PgDn** and **g/G** navigate the focused section. **Enter** on a file opens its diff; **Esc** returns to the file list, then **Esc** returns to the dashboard with the commit selection preserved.

Diffs compare a commit with its **first parent**, clearly labeled for merges. Root commits compare against an empty tree. These reads use immutable commit trees and exclude current index/worktree contents. Deleted, renamed and binary files are supported; empty commits show an empty-file notice. The file list is limited to 5,000 entries and the message preview to 65,536 characters. File patches use the same preview limits as staging diffs. No checkout or repository write is needed to inspect a commit.

## File diffs

Focus **Staging** with **Tab**, select a file with **↑/↓** or **j/k**, then press **Enter**. The diff fills the terminal; **Esc** or **q** returns to the same file selection. **Space** still expands the staging list.

The title identifies **HEAD → index** for staged changes and **index → working tree** for unstaged changes. Untracked files show their contents as additions. Partially staged files have separate entries. Renames use their actual old/new paths. Unresolved conflicts display a notice to review the file in your editor.

Inside the viewer, **↑/↓** or **j/k** scroll, **PgUp/PgDn** move a page, **g/G** jump to the beginning/end, and **←/→** or **h/l** pan long lines. Added lines are green, removed lines bright coral, and hunk headers cyan. `NO_COLOR` is respected.

Only the selected file is diffed, on a separate background worker. The displayed result stays in memory and refreshes on repository changes or **r**. Older results cannot replace a newer view or a different repository. Reads never stage files or update the index. Text files above 256 KiB are omitted, and previews stop at 256 KiB or 4,000 lines; binary files show a notice.

## Switch branches

Use **Tab** to focus **Branches**, then **j/k** or **↑/↓** to highlight a branch. Press **Enter** to switch to it. **›** marks the selection; **●** marks the current branch. Selection stays visible while scrolling and remains on the same branch across refreshes, even when the list reorders. Use **g/G** to jump to the first/last branch and **Space** for a full-size branch list.

Local branches switch normally. Selecting a remote-tracking branch creates a local branch with its upstream set, or switches to the existing local branch if it already tracks that remote ref. If the local name is taken by a different branch, an error explains the conflict; glasspane does not repoint it. No fetch is performed.

Switching uses Git's safe checkout: compatible local edits are preserved, and changes that would be overwritten cause a refusal. There is no automatic stash, discard, force checkout or detached checkout. A branch checked out in another worktree remains protected. Switches run on a background worker; the footer reports progress or success, and errors open a message that **Esc** closes. A finished switch refreshes all panels. Shell mode is unavailable while a switch is in progress.

## Amend the latest commit

Press **`a`** to review an amendment of the current HEAD commit. The dialog shows the branch, commit hash and subject, staged file list, and how many unstaged/untracked entries are excluded. Choose an option with **1/2/3** or **↑/↓**, then press **Enter** to confirm:

| Option | Behavior |
|---|---|
| **1** | Include staged changes and keep the existing message |
| **2** | Edit only the message; existing staged changes stay staged |
| **3** | Include staged changes and edit the message |

Use **PgUp/PgDn** to inspect the staged-file list. **Esc**, `q` or Ctrl+C cancels the review without executing Git. If no changes are staged, only message editing is enabled. Stage files in Staging with **s**, or through `!` shell mode first; amending never stages files automatically or includes unstaged working-tree edits.

Message editing temporarily returns the terminal to your configured Git editor. Git hooks and signing settings are honored. When the command finishes, the dashboard refreshes and reports the old and new hashes. Cancelling or interrupting the editor returns to the dashboard with the result; Git's normal error output remains in terminal scrollback.

Amending rewrites the latest commit. If it has already been pushed, coordinate with collaborators before updating the remote; glasspane never pushes or force-pushes automatically. This shortcut targets HEAD, not an older commit selected elsewhere in the dashboard.

HEAD, the active branch and staged contents are rechecked before execution. A stale review is refused, as are unresolved conflicts or an active merge/rebase/other operation. A minimum terminal size of **60×18** is required to confirm, so a hidden or clipped review cannot execute an amendment. A refresh of index stat-cache metadata alone does not invalidate review.

## Configuration

All settings are optional in `~/.config/glasspane/config.toml` (or `$XDG_CONFIG_HOME/glasspane/config.toml`):

```toml
commit_limit = 50
graph_limit = 200
debounce_ms = 150
branch_sort = "date"       # "date" or "name"; current branch always first
follow_cwd = true
show_remote_head = false
```

Limits and debounce must be positive. Unknown keys and malformed values produce an error. `--commits` and `--no-follow` override the file. `--print-config` prints the effective configuration without starting the terminal UI.

## Repository access

Repository data comes from `git2`. Status queries explicitly disable index updates; snapshot and summary loading remain read-only. Explicit write actions are file staging/unstaging, reviewed new commits through `git commit`, safe branch switching through `git switch`, and reviewed amendment of HEAD through `git commit --amend`. Fetch/push remain available through `!` shell mode. Preparing commit/amend reviews and inspecting commit trees only read repository data. The graph runs only `git --no-pager log`, with optional locks and signature verification disabled, and no network operation. Git lane colors are decoded into structured data; raw ANSI sequences are never rendered. The graph provider is replaceable independently of other repository loading.

File watching uses `notify` and `notify-debouncer-mini`. Git object events, `.lock` files and ignored worktree paths are filtered. Metadata directories outside the worktree are watched for linked worktrees and submodules. If watcher setup or delivery fails, the footer switches to `○ polling` and snapshots refresh every two seconds. Press `?` to see the watcher error.

The UI renders immutable snapshots. A background loader opens its own repository and coalesces pending requests into one latest request; an older directory's result cannot replace a newer one. Relative times redraw every 30 seconds. Terminal raw mode and the alternate screen are restored on normal exit, errors and Rust panics (external `SIGKILL` cannot run cleanup).

## Limitations and next steps

- Remote branches are local remote-tracking refs, **as of your last manual fetch**. The dashboard never fetches or contacts a remote automatically.
- Every relevant change currently reloads the whole snapshot. Very large repositories may take longer to refresh, though Git reads stay off the UI thread.
- File watching can consume resources on large trees, including ignored directories; ignored events are filtered after OS delivery. Network filesystems may silently omit events; press `r` if necessary.
- macOS is the primary platform. Linux is intended to work but has not been validated; Windows is outside v1 scope.
- The graph includes all refs, including tags, matching `git log --all`; a missing Git executable is reported in that panel.
- Non-UTF8 paths are displayed with replacement characters; control characters in repository text are sanitized for display.
- Colors, layout, event timing, startup latency, five-minute idle CPU and terminal restoration still require full application acceptance testing.
- Future work: hunk staging, native graph lanes, ahead/behind counts, worktree listing, and additional actions in the separate `actions` module.
