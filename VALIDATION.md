# Validation record

Date: 2026-11-15. Host: macOS arm64, Rust 1.100.0, zsh.

## Executed successfully

- `cargo fmt --check`: passes for the full crate.
- Standalone graph parser unit test, compiled directly with `rustc --edition 2024 --test src/git/graph.rs`: 3 tests pass. This module has only standard library dependencies, so it can run without Cargo downloads.
- `cargo metadata --no-deps --offline`: manifest and target discovery pass.
- Actual `GitCliGraph` provider compiled directly with rustc and exercised against a disposable repository with diverging branches and a merge: merge lanes, commit subjects and branch decorations appear. Detached HEAD appears too. Separate smoke checks pass for empty repositories and unborn HEAD with history on another branch.
- Before/after comparison for the graph-provider smoke test: no `.git` path's modification time or bytes changed.
- Generated zsh hook executed in `zsh -dfc`: creates an absent state directory, tracks a path with spaces (including a trailing space), and remains functional after being sourced twice.

## Full Cargo checks

After dependencies became available, the first compilation exposed a malformed Clap attribute on `init zsh`. It is corrected to `#[arg(value_parser = ["zsh"])]`; the existing CLI regression test covers valid and invalid shells.

- `cargo test --offline --all-targets`: all 57 tests pass.
- `cargo clippy --offline --all-targets -- -D warnings`: passes.
- `cargo fmt --check`: passes.
- `cargo build --offline --release`: passes.
- `Cargo.lock` is generated and retained.
- Release CLI smoke checks: `--version`, `--print-config`, and `init zsh` pass.
- Release dashboard launched in a pseudo-terminal and exited successfully with both `q` and Ctrl+C; terminal attributes matched their pre-launch values afterward.

Native FSEvents reports `unable to start FSEvent stream` in this sandbox. The live-refresh integration test verifies the two-second polling fallback here; it exercises native watching when the host permits it. The full snapshot safety test passes its metadata byte/mtime comparison and 100-microsecond lock sampler. Native transient lock event observation remains unverified here. No tests are skipped.

To repeat the checks:

```sh
./scripts/check.sh
```

## Passing automated coverage

- Repository discovery, empty/unborn branches and non-repository errors.
- Recent history limits, current-first branch sorting and detached HEAD.
- Partial staging, ignored files, untracked files, staged/unstaged renames.
- A local bare origin with test-only filesystem push/fetch, remote HEAD exclusion and upstream display.
- Merge conflicts, rebase, cherry-pick and bisect state.
- Linked worktree `.git` files and shared metadata directory filtering.
- Full snapshot read-only check: metadata bytes/mtimes and transient `index.lock` observation.
- Structured graph parsing, ANSI lane-color decoding, and a real colored merge graph invocation.
- Unicode connectors, aligned columns, branch labels, wide Unicode/emoji truncation, NO_COLOR, and expansion/restoration of a graph with commit data.
- Watcher filters and live worktree refresh via the available native or polling backend.
- Background loader coalescing and latest-directory result handling.
- CLI argument parsing, configuration serialization and malformed configuration.
- Zsh hook execution and cwd paths with spaces.
- UI rendering at zero, tiny, narrow and wide terminal sizes, help, scrolling and control-character sanitization.

## Manual acceptance checklist (pending)

Use a disposable repository and a second terminal. Keep the dashboard open with `cargo run -- /path/to/repo`.

- [ ] Initial repository panels populate within 500 ms on a mid-size repository.
- [ ] Edit a tracked file: UNSTAGED refreshes without a keypress.
- [ ] Create an untracked file: UNTRACKED refreshes.
- [ ] Stage a file in the second terminal: STAGED updates within about 0.5 seconds.
- [ ] Commit: history, graph and staging refresh together.
- [ ] Switch branch: header, refs and history update.
- [ ] Fetch manually: remote refs and FETCH_HEAD time update; dashboard performs no fetch.
- [ ] Create a merge conflict: CONFLICTED and MERGING appear.
- [ ] Test unborn branch, detached HEAD, rebase and bisect displays.
- [ ] Source the zsh hook; change into a subdirectory, a different repository and a non-repository directory.
- [ ] Launch outside a repository; it stays open and can follow into a repository.
- [ ] Resize through grid, expanded-panel and minimal terminal sizes; help and scrolling remain usable. Automated layout coverage includes expansion and restoration.
- [x] Quit with `q` and Ctrl+C in a pseudo-terminal; process exits successfully and terminal attributes restore. Visual cursor behavior remains part of the manual terminal check.
- [ ] Debug panic: `GLASSPANE_DEBUG_PANIC=1 cargo run -- /path/to/repo`; terminal restores before the panic is reported.
- [ ] Idle for five minutes and inspect CPU usage in Activity Monitor or `top`.
- [ ] Observe `.git` during use and verify glasspane causes no writes or `index.lock` creation.
- [ ] Test watcher failure/polling behavior and linked-worktree shared-ref refreshes.
- [ ] Repeat rendering with `NO_COLOR=1`.

## Shell handoff validation — 2026-11-17

- `cargo test --offline --all-targets`: all 32 Rust tests pass.
- Formatting, Clippy with warnings denied, and release build pass.
- `python3 scripts/check-shell.py target/release/glasspane`: three end-to-end scenarios pass using a controlling pseudo-terminal and disposable repositories.
- Normal flow: open shell in a path with spaces, stage a file, invoke `git commit` with a configured editor that verifies it has a terminal, exit, and observe the new commit in the dashboard.
- Interrupt/EOF flow: Ctrl+C interrupts a foreground shell job without killing the dashboard; resizing during shell mode and returning with Ctrl+D repaint the dashboard; quitting restores terminal attributes.
- Error flows: a nonexistent shell executable and an interactive shell exiting with code 7 both return to the dashboard; final quit restores terminal attributes.

The shell test harness drains terminal output while waiting for exit and waits for a foreground job before sending Ctrl+C, avoiding output-buffer blocking and startup races in a pseudo-terminal. Native FSEvents remains unavailable in the sandbox; the dashboard's existing polling fallback is used during these tests.

## Compact terminal dashboard validation

- All 35 Rust tests pass, including compact and narrow Unicode layouts, partially staged counts, the noninteractive `summary` CLI and non-repository handling.
- The `.git` byte/mtime and lock-sampling safety check now covers both full snapshots and the lightweight summary loader.
- The pseudo-terminal shell checks verify the initial unborn-branch card, summary refresh after staging and committing, branch changes, leaving/re-entering a repository, preservation of `.zshenv`, aliases and custom ZDOTDIR, previous command exit status, and a final card on quitting.
- A fourth pseudo-terminal scenario verifies normal HOME startup with ZDOTDIR unset; all four scenarios pass against the release binary.
- Formatting, Clippy with warnings denied and the release build pass.

## Selectable branch checkout validation

- All 47 Rust tests pass; formatting, Clippy with warnings denied and the release build pass.
- Actions cover local branch checkout and worktree contents, staged/unstaged overwrite refusal with the original index bytes retained, compatible tracked changes and untracked files, branches checked out in another worktree, and rebase refusal.
- Remote actions cover tracking creation, reuse of a matching local upstream, name-collision refusal, no fetch, no partial branch creation on failed checkout, and symbolic/deleted target refusal.
- A post-checkout hook failure test verifies that the message reports the branch Git actually switched to.
- Selection tests cover grouped-list navigation, keeping the highlighted row visible, returning to the first row, stable identity after reordering, fallback when a branch disappears, and blocking requests from stale snapshots or during another switch.
- The pseudo-terminal script now has a fifth scenario that sends Tab, j and Enter to the real dashboard, verifies that HEAD changes, and checks that the normal-terminal summary reports the new branch. That scenario supplies Git environment overrides pointing at a second repository and verifies its HEAD and index are untouched. Existing shell/terminal scenarios still pass.
- The read-only snapshot/summary safety test remains unchanged in purpose and continues to pass; only an explicit user action requests checkout.

## Commit amendment validation — 2026-11-17

- All 57 Rust tests pass; formatting, Clippy with warnings denied, and release build pass.
- Nine amend integration tests cover staged-only amendment with message/author preservation, message-only amendment with tree/index preservation, combined editing with parent preservation, unborn/operation/conflict refusal, stale HEAD/ref/index rejection, metadata-only index refresh acceptance, cancelled editors, failed pre-commit hooks, and no-staged-change mode gating.
- The UI test covers mode labels, rewrite warning, file inclusion/exclusion, staged-file paging at large and minimum sizes, and the small-terminal guard.
- The existing read-only `.git` byte/mtime and index-lock observation test now includes preparing amend reviews.
- `python3 scripts/check-amend.py target/release/glasspane` passes seven controlling-pty scenarios: each of the three modes, Esc cancellation, Ctrl+C in an editor, a changed HEAD during review, and Enter in a small terminal. It checks actual commit trees/messages, staged versus unstaged preservation, index-lock cleanup, dashboard survival, and terminal restoration. Fake editors explicitly require tty input.
- `python3 scripts/check-shell.py target/release/glasspane` still passes all five existing shell/checkout scenarios after terminal-handoff and SIGINT changes.
- The harness allows the standalone Escape encoding to resolve before another key and drains redraw output; narrow final cards are recognized separately from wide cards. These changes avoid Alt-key ambiguity and false test failures from terminal display diffs.
- Real GPG/signing-agent interaction has not been manually exercised; the native Git command retains signing configuration and interactive terminal I/O.

## Full-screen file diffs

- `./scripts/check.sh`: all 63 Rust tests, formatting, Clippy with warnings denied, and release build passed.
- `python3 scripts/check-diff.py target/debug/glasspane`: passed on a controlling pseudo-terminal. Covers Staging selection, staged/unstaged previews, external-edit refresh, resizing, return to the selected file, and terminal restoration.
- Diff tests cover partial staging, unchanged index bytes/mtime and HEAD, unborn branches, deletions, binary/untracked files, literal pathspecs, renamed paths, bounded previews, stale-result rejection, and small-terminal rendering.
- Text previews omit files above 256 KiB and stop at 4,000 lines or 256 KiB of patch data. Conflicted files show an editor-review notice.

## File staging, commit dialog, and commit inspection

- `./scripts/check.sh` passes all 79 Rust tests, formatting, Clippy with warnings denied, and the release build.
- Staging tests cover exact literal paths, additions/deletions/renames, partially staged files, unborn-branch unstaging with working-file preservation, stale selections/changed branches, conflict refusal, and existing index locks.
- Commit tests cover initial and subsequent commits, staged-only inclusion with working edits preserved, multiline/Unicode messages and editing, empty-message rejection, stale branch/index refusal, operation/conflict refusal, hook failure/retry, and normal/small-window rendering.
- Inspection tests cover root/normal/merge/empty commits, rename/deletion/binary entries, immutable tree diffs, unchanged index bytes/mtime and HEAD, small windows, stable selection, stale-result rejection, and modal preparation guards. Merge inspection compares with the first parent.
- `python3 scripts/check-workflow.py target/release/glasspane` passes seven controlling-pty scenarios: stage/unstage/commit/inspect flow, cancellation, empty message, stale index, small-window refusal, hook failure with message-preserving retry, and Ctrl+C during a hook. Terminal state and index-lock cleanup are checked.
- Existing shell (five scenarios), amend (seven scenarios), and full-screen diff terminal checks pass against the updated application. The amend checks were repeated against the final release binary.
- The workflow harness drains pending message input before injecting resize events so its confirmation checks execute at the intended terminal size.
- Real signing-agent interaction has not been manually exercised; native Git retains signing settings and terminal I/O. Conflict resolution and individual hunk staging remain outside this feature set.

## glasspane / glass rename

- All 81 Rust tests, formatting, Clippy with warnings denied, and the release build pass.
- Installed into a disposable directory with `cargo install --offline --path . --root ...`; Cargo installed both `glass` and `glasspane`, and both report `glasspane 0.1.0`.
- CLI integration tests verify equivalent help, summary and zsh-init behavior under both executable names. Configuration tests verify legacy fallback, new-config precedence, invalid-config errors, and preservation of the legacy file.
- Release `glass` passes all five shell/checkout scenarios and all seven staging/commit/inspection scenarios. Release `glasspane` passes all seven amend scenarios and the diff terminal scenario.
- Existing zsh startup, custom ZDOTDIR, shell summaries and terminal restoration remain covered after renaming the shell integration variables and hooks. Update the opt-in cwd hook in `.zshrc` to `eval "$(glass init zsh)"` when upgrading.

## Startup directory regression

- Startup always uses the calling cwd or explicit positional path, rather than applying saved shell-tracking state.
- `scripts/check-startup.py` passes four controlling-pty scenarios across `glass` and `glasspane`: no path and explicit path, with a saved cwd of `/`. It verifies the launch repository, the directory inherited by shell mode, subsequent live directory following, normal exit and terminal restoration.
- Temporary test paths are resolved to account for macOS `/var` versus `/private/var` aliases.
- All 81 Rust tests, formatting, Clippy with warnings denied and release build pass. All five existing shell integration scenarios also pass.
