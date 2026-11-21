# Implementation decisions

## Git backend: git2

Use `git2` with vendored libgit2, with default transport features disabled. This follows the spec's default and keeps status, refs, history and operation-state reads inside one API. No core data is collected with Git subprocesses. Status options explicitly set `update_index(false)`. A dedicated test compares `.git` bytes and modification times across full snapshots and observes for transient `index.lock` creation. That test passes; native FSEvents observation is unavailable in the sandbox, so the lock sampler and metadata comparison provide the executed coverage.

The manifest uses the current stable major/minor crate release lines checked against official crate documentation during implementation (including git2 0.21 and Ratatui 0.30). Dependencies have now been resolved and `Cargo.lock` records the exact versions used by the successful build and tests.

## Branch graph: Option A, Git CLI

Use `git log --graph --all --decorate=short`, with explicit graph colors and unit separators between the graph, hash, decorations and subject. This retains Git's merge routing and lane color assignment, including branches that move across columns, without building a native lane algorithm. `GraphProvider` separates the implementation from snapshot loading. The Git executable is probed by the first graph load; errors stay in that panel and do not prevent other panels from loading.

The display replaces ASCII graph characters with Unicode connectors, uses Git's lane colors, dims hashes, aligns subjects and places branch labels in a separate column. The current HEAD node and label are emphasized. Git's `--color=always` is an intentional change from the original brief: its ANSI graph colors are decoded into typed lane cells, not passed through to the terminal. The UI still respects `NO_COLOR`. Structured fields ensure parentheses in commit subjects are never mistaken for branch labels. Long labels and subjects are clipped by terminal cell width and grapheme boundaries.

`--all` matches the spec's recommended invocation and includes tags as well as local and remote branches. It also handles an unborn current branch while another branch still has history. Detached HEAD is included. `GIT_OPTIONAL_LOCKS=0`, `--no-pager`, `--no-show-signature` and `core.fsmonitor=false` keep the graph invocation read-only and avoid pager/signature/fsmonitor integrations.

## Concurrency and coalescing

Use standard library threads, channels, a mutex and a condition variable; no async runtime. The UI polls terminal input at 250 ms intervals, drains application events, and redraws only on changes or every 30 seconds for relative times. A loader mailbox stores at most one pending request while one is running. Cwd generations reject superseded results. Watch configuration is replaced only when repository paths change, preventing refresh gaps on ordinary edits.

The watcher and debouncer own repository handles separate from the loader. Callback delivery is bounded, ignored paths are filtered before enqueueing, and overflow requests a full refresh. The UI never performs Git I/O. Full refreshes are the v1 fallback rather than targeted partial snapshot updates.

## Watcher and directory tracking

Use `notify` plus `notify-debouncer-mini`, with default 150 ms debounce. Watch the worktree plus metadata directories not already covered, including libgit2's shared common directory for linked worktrees. Filter objects, locks and ignored files. Watch the cwd file's parent to support atomic replacement, and periodically check for a previously missing state directory. On watcher errors, use a two-second polling fallback and expose the error in help.

Do not create state directories from the dashboard; the generated opt-in zsh hook creates them when it writes. This avoids filesystem writes on ordinary application startup. Last shell writer wins. Explicit CLI paths win over stale state at startup, and subsequent cwd changes follow normally. Invalid or transient empty state reads do not switch directories; deleting the state file returns to launch-directory mode.

## Responsive layout and terminal behavior

Use the requested four-panel grid at 100+ columns when there is enough vertical space. Otherwise show one focused full-width panel, keeping all content accessible via Tab. Space expands the focused panel even on wide terminals, and Space or Esc restores the grid. Enter also expands other panels, while in Branches it switches the selected branch. Below 30×12, display a resize prompt. Clamp scrolling to the content viewport rather than allowing unlimited blank scrolling.

Install a panic hook and a scoped terminal guard. The guard restores the terminal before waiting for the loader during shutdown. The event loop exits if a background worker stops unexpectedly. A debug-only `GLASSPANE_DEBUG_PANIC` environment switch allows manual panic-cleanup verification. Respect `NO_COLOR` and replace control characters in displayed repository content.

## Configuration paths

Use `$XDG_CONFIG_HOME/glasspane/config.toml` with a `~/.config` fallback even on macOS, matching the brief rather than macOS Application Support conventions. The state file similarly honors `$XDG_STATE_HOME` with a `~/.local/state` fallback. Empty XDG variables use the fallback. No config file is created implicitly.

## Red contrast adjustment

Use bright coral (`#ff8080`) for red graph lanes, remote branch labels, conflicts and operation-state indicators. Remote labels are no longer dimmed, keeping them readable against black terminal backgrounds. `NO_COLOR` continues to suppress foreground colors.

## Explicit shell handoff (user-requested extension)

Pressing `!` releases raw mode and the alternate screen and starts the user's interactive `$SHELL` in the displayed cwd. This extends the original display-only scope at the user's request so they can stage and commit in the same terminal. Repository loaders remain read-only; safe branch switching was subsequently added as an explicit action, and there are no automatic fetches. Commands are typed directly by the user, rather than constructed or interpreted by the dashboard.

The shell inherits terminal I/O and loads its usual interactive configuration. `exit` or Ctrl+D returns to the UI. The UI then re-enables raw mode, enters the alternate screen, resizes the fullscreen viewport to invalidate the old frame buffer, and requests a full snapshot. Explicit viewport resize avoids a cursor-position query that `Terminal::clear()` performs and supports terminals that do not answer that query. Shell startup errors are recoverable and appear at the top of help; terminal restoration errors follow the normal fatal-error cleanup path. Nonzero shell exit codes do not terminate the dashboard.

`GLASSPANE_SHELL=1` is set in the child environment for optional prompt/startup customization. Shell cwd changes reach the dashboard through the existing opt-in cwd hook; they do not change the parent process's working directory implicitly.

A Python pseudo-terminal integration script verifies a real Git commit through the shell with a configured editor and path names containing spaces, automatic history refresh on return, Ctrl+C during a foreground command, EOF, resizing, failed startup, nonzero exit, and terminal attribute restoration. Python is a test dependency only.

## Compact terminal dashboard

Show a compact branch, latest-commit and staging-count card when entering shell mode and after a successful normal TUI exit. `glasspane summary [PATH]` also prints it directly without raw mode, file watching, graph loading, or loading branch/history lists. It shares the HEAD and read-only status readers with the full dashboard. It honors terminal width, sanitizes control characters, clips Unicode at grapheme boundaries, and suppresses colors for redirected output and `NO_COLOR`.

In zsh, refresh the card from a child-shell `precmd` hook after each command. Do not draw asynchronously over the command line. Use a temporary `ZDOTDIR` shim that sources the user's original `.zshenv` and `.zshrc`, restores the user's `ZDOTDIR` before their `.zshrc`, and appends the hook. Preserve the preceding command's exit status and any user-defined aliases or prompt. Remap a global-startup HISTFILE based on the temporary directory to its normal destination. The shim is removed after the child exits; no user shell files are edited. Other shells get a one-time initial summary through the helper subprocess.

The summary helper performs its Git reads in a separate process during shell use, keeping Git I/O out of the TUI event loop. After the TUI has restored the terminal and stopped its workers, the final card loads a fresh summary rather than reusing a possibly stale snapshot. A non-repository directory produces a friendly one-line message.

## Selectable branches and safe switching (user-requested extension)

Branches are selectable independently of their visual group headers. Store the selection as a name plus local/remote identity rather than a row index, preserving it across sorting and refreshes. Scroll the viewport to keep the selected row visible; distinguish selection (`›` plus reverse styling) from current HEAD (`●` plus green bold styling). Enter switches the selected branch. Space expands/restores any panel, retaining Enter expansion elsewhere.

Place repository writes in `actions.rs`, separate from Git loaders. Use native `git switch` with no force, no guessing, and no recursive submodule checkout. Native Git supplies dirty-file, operation-state and other-worktree safeguards. Run it on a serialized background worker and do not accept another switch or enter shell mode until it finishes. Clear inherited Git repository/index routing variables before invoking the write, so a Git hook/helper environment cannot redirect checkout to a different repository. Requests capture the displayed snapshot's repository root and cwd generation; stale rows cannot dispatch actions against a previously displayed repository. Refresh data after completion, and show errors without dropping the dashboard. Terminal cleanup happens before waiting for an ongoing action on exit.

For remote refs, infer the local name using libgit2's configured remote mapping. Reuse a local branch only if its upstream matches; otherwise refuse a name collision. If absent, create a tracking branch through Git's safe checkout. Symbolic remote HEAD refs are not checkout targets. No fetch, force, stash, reset, overwrite or detached checkout is used. Normal Git checkout hooks are honored. A failed post-checkout hook can occur after HEAD has moved; check the resulting HEAD and report the switch plus Git's warning accurately rather than claiming the old branch remains active.

## Reviewed latest-commit amendment

`a` prepares an immutable review of HEAD and staged files on the existing action worker. The review offers staged changes with `--no-edit`, message-only amendment with `--only --edit`, or staged changes plus `--edit`. It never invokes `git add`, includes unstaged edits, uses `--no-verify`, resets authorship, or pushes. Native Git manages the commit, configured editor, hooks and signing.

The plan records the full HEAD OID, HEAD ref identity and semantic index entries (path, object ID, mode and flags). Revalidate before invoking Git and refuse changed HEAD, branch or staged contents. Ignore only stat-cache changes, which do not change staged content. Preparing a review does not write trees, objects or the index. Reuse the environment-clean Git command builder so inherited repository/index overrides cannot redirect the amendment. Refuse bare/unborn repositories, conflicts and ongoing Git operations. Native Git remains responsible for locking and hook behavior during execution.

The modal explicitly confirms the history rewrite, supports paging through all staged files, and requires at least 60×18 before Enter can execute. Cwd changes cancel an open review, generation checks reject stale prepared plans, and branch switching/shell launch cannot run during preparation or an open review. With no staged changes, default to and enable only the message option.

Reuse the terminal handoff for every amend mode, including `--no-edit`, so interactive hooks and signing prompts have a tty. During handoff the parent temporarily ignores SIGINT while exec children restore its default disposition; Ctrl+C reaches Git/editor without terminating the waiting dashboard. Restore the previous signal disposition after the terminal is back in raw alternate-screen mode. Post-fork setup uses only signal syscalls. No subprocess command string is built or evaluated.

Compare resulting HEAD after Git exits. Report old/new hashes on success, unchanged HEAD on failure, or an explicit review warning if Git failed but HEAD changed (for example due to another process or hook). Do not roll back or discard staged/working changes automatically.


## glasspane command naming

Rename the Cargo package and application branding to `glasspane`. Install both `glasspane` and the short `glass` command as thin entry points into the same library CLI implementation, so behavior cannot diverge and no shell alias is required. Keep `glasspane` as Cargo's default run target. New configuration and cwd state use the `glasspane` directory; legacy `gitpane/config.toml` remains a read-only fallback when the new config is absent. A new cwd hook removes the old callback when sourced in an existing zsh session. User shell files are never edited automatically.


## Launch directory takes precedence over saved cwd state

Start in the calling process's current directory unless a positional PATH is supplied. Never apply the saved cwd tracking file during startup: it can belong to an earlier session or another shell and point outside the intended repository. The watcher already records the initial saved value as its baseline and emits only subsequent changes, preserving live following and removal-to-launch behavior.
