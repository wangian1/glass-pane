# SPEC: `glasspane` — Live Git Dashboard TUI (Rust)

> `glasspane` is a placeholder name. Rename freely.
> This document is the build brief for an AI coding agent. Follow it in order. Where something is marked **DECISION FOR AGENT**, pick the option, document why in `DECISIONS.md`, and move on.

---

## 1. Goal

A read-only, full-screen terminal dashboard that the user leaves open in a separate terminal window/tab. It shows the current state of a Git repository **live**, with no commands needed after launch:

- Current branch and its recent commits
- A branch graph
- Local branches and remote-tracking branches (`git branch -r` equivalent)
- Staging area: staged, unstaged, untracked, conflicted files
- Repo root path and the user's current working directory

It updates automatically when the repo changes (commit, checkout, stage, fetch, edit a file).

Think "always-on lazygit, display-only."

---

## 2. Target environment

| Item | Value |
|---|---|
| OS | macOS (primary). Linux should work but is not a v1 test target. Windows out of scope. |
| Shell | zsh |
| Terminal | Any modern terminal (Terminal.app, iTerm2, WezTerm, Ghostty, Alacritty) |
| Language | Rust, stable toolchain, 2021 edition or later |
| Distribution | `cargo install --path .` for v1 |

---

## 3. Scope

### In scope (v1)
- Read-only TUI dashboard
- Live refresh via file watching
- Local data only — **never** runs `git fetch` or touches the network
- Optional zsh hook so the dashboard follows the shell's current directory (see §6.6)

### Out of scope (v1) — listed in Roadmap (§11)
- Any write action (stage, unstage, checkout, commit, fetch, push)
- Auto-fetch / network access
- Ahead/behind counts
- "Mine vs others" commit filtering
- File tree view, worktree listing
- Windows support

**Hard rule:** v1 must never modify the repository, the index, or refs. It must not create `index.lock` or any other file inside `.git/`.

---

## 4. Tech stack

Use the current stable version of each crate at build time. Do not pin to versions from this document.

| Purpose | Crate | Notes |
|---|---|---|
| TUI rendering | `ratatui` | Widgets, layout |
| Terminal backend | `crossterm` | Raw mode, alternate screen, input events |
| Git access | `git2` (libgit2 bindings) | Status, refs, revwalk. See note below. |
| File watching | `notify` + `notify-debouncer-full` (or `notify-debouncer-mini`) | FSEvents backend on macOS |
| CLI args | `clap` (derive) | |
| Errors | `anyhow` (bin), `thiserror` (lib types) | |
| Config | `serde` + `toml` | |
| Config path | `directories` | `~/.config/glasspane/config.toml` |
| Time formatting | `chrono` or `time` | Relative dates ("3h ago") |

**DECISION FOR AGENT — git backend:** `git2` is the default. `gix` (gitoxide) is an acceptable alternative if it simplifies the build. Do not shell out to `git` for core data (status, refs, log), except optionally for the branch graph (§6.3).

**Index safety note:** `git2` status calls can try to refresh/write the index. Open the repo and query status in a way that does not write the index (e.g. avoid `index.write()`; check whether status options trigger an index update and disable it). Verify by watching `.git/` during a test run — no `index.lock` should appear.

---

## 5. UX / Layout

Default layout (resizable, responsive to terminal size):

```
┌─ Header ───────────────────────────────────────────────────────────────┐
│ repo: ~/code/myproj   cwd: ~/code/myproj/src/api   branch: feat/login │
│ HEAD: a1b2c3d "Add login form"  upstream: origin/feat/login            │
├─ Branch Graph ──────────────────────────┬─ Branches ───────────────────┤
│ * a1b2c3d (HEAD -> feat/login) Add ...  │ LOCAL                        │
│ * 9f8e7d6 Wire up auth service          │ ▶ feat/login      a1b2c3d 2h │
│ | * 4c5d6e7 (origin/main) Fix typo      │   main            4c5d6e7 1d │
│ |/                                      │ REMOTE                       │
│ * 3b2a190 Initial commit                │   origin/main     4c5d6e7 1d │
│                                         │   origin/feat/... a1b2c3d 2h │
├─ Recent Commits (feat/login) ───────────┼─ Staging ────────────────────┤
│ a1b2c3d  Ian   2h  Add login form       │ STAGED (2)                   │
│ 9f8e7d6  Ian   5h  Wire up auth service │  M src/api/auth.rs           │
│ 3b2a190  Ian   3d  Initial commit       │  A src/api/login.rs          │
│                                         │ UNSTAGED (1)                 │
│                                         │  M README.md                 │
│                                         │ UNTRACKED (1)                │
│                                         │  ? notes.txt                 │
└─────────────────────────────────────────┴──────────────────────────────┘
 [Tab] focus panel  [j/k] scroll  [r] refresh  [?] help  [q] quit   ● live
```

### Panels

1. **Header** — repo root, cwd, current branch (or `DETACHED @ <hash>`), HEAD commit short hash + subject, upstream name if set. Special states shown prominently: `MERGING`, `REBASING`, `CHERRY-PICKING`, `BISECTING`.
2. **Branch Graph** — see §6.3.
3. **Branches** — two groups: LOCAL and REMOTE. Each row: name, tip short hash, relative date. Current branch marked. Remote group = remote-tracking refs (`refs/remotes/*`), excluding `*/HEAD` symbolic refs.
4. **Recent Commits** — last N commits (default 50) on the current branch: short hash, author name, relative date, subject.
5. **Staging** — groups: CONFLICTED, STAGED, UNSTAGED, UNTRACKED. Each row: status letter + path relative to repo root. Renames shown as `old → new`. Empty groups hidden. Counts in group headers.
6. **Footer** — keybinds and a live indicator (● live / ○ watcher error).

### Keybinds (v1, navigation only)

| Key | Action |
|---|---|
| `Tab` / `Shift+Tab` | Cycle panel focus |
| `j`/`k`, `↓`/`↑` | Scroll focused panel |
| `g` / `G` | Top / bottom of focused panel |
| `r` | Force full refresh |
| `?` | Toggle help overlay |
| `q`, `Ctrl+C` | Quit and restore terminal |

### Colors
- Current branch: bold/green
- Remote branches: red/dim
- Staged: green, Unstaged: yellow, Untracked: grey, Conflicted: bold red
- Respect `NO_COLOR` env var

---

## 6. Functional requirements

### 6.1 Repo discovery
- `glasspane` with no args: discover repo from the launch directory, walking up (like `git`).
- `glasspane <path>`: discover from `<path>`.
- Not in a repo: show a clear "Not a git repository" screen, keep running, and retry discovery when the cwd changes (via §6.6) — do not crash.
- Support repos where `.git` is a file (worktrees/submodules) via libgit2's normal discovery.

### 6.2 Recent commits
- Revwalk from HEAD, topological + time order, limit N (config).
- Unborn branch (no commits yet): show "No commits yet".

### 6.3 Branch graph
Show all local branches + remote-tracking branches, ASCII/Unicode lane graph, decorated with ref names, limited to N commits (default 200).

**DECISION FOR AGENT — graph implementation:**
- **Option A (recommended for v1):** shell out to
  `git log --graph --all --decorate --color=never --format=<fmt> -n <N>`
  and parse the output into styled ratatui lines. Fast to ship, matches what users already know. Read-only, so it's safe. Requires `git` on PATH — check at startup and show a message in the panel if missing.
- **Option B:** implement a lane-assignment algorithm over a `git2` revwalk. More work, no external dependency.

Choose A unless it causes problems. Isolate the graph behind a trait so B can replace it later.

### 6.4 Branches
- Local: `refs/heads/*`. Remote: `refs/remotes/*`.
- Sort: current branch first, then by tip commit date descending (config option: alphabetical).
- Data comes **only** from local refs. Show a small note in the panel title: `REMOTE (as of last fetch)`. If available, show the mtime of `.git/FETCH_HEAD` as "last fetched 3h ago".

### 6.5 Staging
- Use libgit2 status with untracked files included, ignored files excluded, rename detection on (HEAD↔index and index↔workdir).
- A file can appear in both STAGED and UNSTAGED (partially staged) — show it in both.
- Paths relative to repo root.
- Large repos: status must not block the UI thread (see §7).

### 6.6 Current directory tracking (zsh)
A separate TUI cannot see another shell's cwd. Solve with an opt-in zsh hook:

- `glasspane init zsh` prints a snippet the user adds to `~/.zshrc`:
  ```zsh
  # glasspane cwd tracking
  _glasspane_chpwd() { print -r -- "$PWD" >| "${XDG_STATE_HOME:-$HOME/.local/state}/glasspane/cwd" }
  autoload -Uz add-zsh-hook
  add-zsh-hook chpwd _glasspane_chpwd
  _glasspane_chpwd
  ```
  (Agent: create the directory if missing; verify the snippet works in zsh.)
- The dashboard watches that state file. When it changes:
  - Update `cwd` in the header.
  - If the new cwd is in a **different** repo, switch the whole dashboard to that repo.
- `--no-follow` flag disables this. Without the hook installed, `cwd` = launch directory, and the header shows `cwd: (launch dir)`.
- Multiple shells: last writer wins. Acceptable for v1; note it in README.

### 6.7 Live refresh
- Watch recursively:
  - The repo's git dir (`.git/`): `HEAD`, `index`, `refs/`, `packed-refs`, `FETCH_HEAD`, `MERGE_HEAD`, `rebase-merge/`, `rebase-apply/`, etc.
  - The working tree (for unstaged/untracked changes).
  - The cwd state file (§6.6).
- **Ignore** events from `.git/objects/`, `*.lock` files, and paths ignored by `.gitignore` (use libgit2 `status_should_ignore` or the `ignore` crate) to avoid event storms.
- Debounce events (default 150 ms, configurable).
- Map events to targeted refreshes where cheap (e.g. `refs/` change → branches + graph + commits; workdir change → staging only). A full refresh is acceptable as a fallback.
- Fallback: if the watcher fails, poll every 2 s and show `○ polling` in the footer.
- `r` always forces a full refresh.

---

## 7. Architecture

```
src/
  main.rs          # arg parsing, setup, terminal init/restore
  app.rs           # App state, event loop, focus/scroll state
  event.rs         # unified event enum: Key, Resize, RepoChanged(kind), CwdChanged, Tick
  git/
    mod.rs         # RepoSnapshot struct + loader
    status.rs      # staging data
    refs.rs        # local/remote branches
    log.rs         # recent commits
    graph.rs       # GraphProvider trait + git-CLI impl
    state.rs       # merge/rebase/detached detection
  watch.rs         # notify watcher, filtering, debounce -> events
  cwd.rs           # zsh state file read/watch, `init zsh` snippet
  ui/
    mod.rs         # layout
    header.rs, graph.rs, branches.rs, commits.rs, staging.rs, footer.rs, help.rs
  config.rs
```

### Threading model
- **UI thread:** owns terminal, renders, handles input. Never does git I/O.
- **Watcher thread:** produces debounced `RepoChanged`/`CwdChanged` events into a channel.
- **Loader worker:** receives refresh requests, builds a new `RepoSnapshot` (opening its own `git2::Repository` — `Repository` is not `Sync`), sends it back. Coalesce requests: if a load is running, queue at most one more.
- Use `std::sync::mpsc` or `crossbeam-channel`. Async runtime (tokio) is **not** required; don't add one unless justified in `DECISIONS.md`.

### Data model (sketch)
```rust
struct RepoSnapshot {
    root: PathBuf,
    head: HeadInfo,              // branch name | detached hash | unborn
    upstream: Option<String>,
    state: RepoState,            // Clean, Merging, Rebasing, ...
    local_branches: Vec<BranchInfo>,
    remote_branches: Vec<BranchInfo>,
    last_fetch: Option<SystemTime>,
    commits: Vec<CommitInfo>,
    graph: Vec<GraphLine>,
    staging: Staging,            // conflicted, staged, unstaged, untracked
    loaded_at: Instant,
}
```
UI renders purely from the latest snapshot. Keep scroll positions stable across refreshes where possible.

### Terminal hygiene
- Alternate screen + raw mode on start; **always** restore on exit, on error, and on panic (install a panic hook).
- Handle resize events.
- Target idle CPU ≈ 0% (no busy loop; block on the event channel with a tick timeout ≥ 250 ms for relative-time updates).

---

## 8. Configuration

`~/.config/glasspane/config.toml` (all optional):

```toml
commit_limit = 50
graph_limit = 200
debounce_ms = 150
branch_sort = "date"      # "date" | "name"
follow_cwd = true
show_remote_head = false
```

CLI flags override config. `glasspane --print-config` dumps effective config.

---

## 9. CLI

```
glasspane [PATH]              Launch dashboard for repo at PATH (default: cwd)
glasspane init zsh            Print zsh hook snippet for cwd tracking
  --no-follow               Ignore the zsh cwd state file
  --commits <N>             Override commit_limit
  --print-config            Print effective config and exit
  -h, --help / -V, --version
```

---

## 10. Testing & acceptance

### Tests
- Unit tests for each `git/` module using temp repos built with `git2` in `tempfile` dirs: empty repo, unborn branch, detached HEAD, partial staging, renames, merge conflict, rebase-in-progress, repo with remote-tracking refs (create a bare repo as "origin" and fetch from it in the test setup — that's test-only network-free fetching).
- Graph parser tests from captured `git log --graph` fixtures.
- Watcher filter tests: `.git/objects/*` and `*.lock` ignored; `.git/HEAD` and workdir files pass.
- Read-only test: run a full snapshot load and assert no file under `.git/` changed mtime and no `index.lock` was created.

### Manual acceptance checklist (macOS, zsh)
1. Launch in a repo → all panels populated in < 500 ms on a mid-size repo.
2. In another tab: `git add file` → file moves to STAGED within ~0.5 s, no keypress.
3. `git commit` → commits, graph, staging update.
4. `git checkout other-branch` → header/branches/commits update.
5. `git fetch` (run manually) → REMOTE list and "last fetched" update.
6. Edit a tracked file → appears in UNSTAGED.
7. Create a merge conflict → CONFLICTED group + `MERGING` header badge.
8. With zsh hook: `cd` into a subdir → header cwd updates; `cd` into a different repo → dashboard switches repos.
9. Launch outside a repo → friendly message, no crash.
10. Resize terminal → layout adapts, no panic.
11. Quit with `q`, `Ctrl+C`, and a forced panic (debug build) → terminal fully restored.
12. Idle for 5 min → near-zero CPU.
13. No `index.lock` ever appears during any of the above caused by glasspane.

### Definition of done
- `cargo build --release` clean; `cargo clippy -- -D warnings` clean; `cargo fmt --check` clean; all tests pass.
- README with install, usage, zsh hook setup, keybinds, known limitations.
- `DECISIONS.md` listing every **DECISION FOR AGENT** choice and why.

---

## 11. Roadmap (not v1 — design so these fit later)

- **v2 actions:** stage/unstage file, switch branch, manual fetch (keybind), with confirmation prompts. Keep a clear separation between read-only `git/` loaders and a future `actions/` module.
- Opt-in periodic auto-fetch with interval in config.
- Ahead/behind counts vs upstream.
- Highlight/filter "my" commits by `user.email`.
- File tree view with change markers; worktree listing.
- Diff preview pane for selected file/commit.
- Native lane-graph implementation (Option B in §6.3).
- Linux as a tested target; Windows support.

---

## 12. Build order for the agent

1. Scaffold crate, CLI, config, terminal init/restore + panic hook.
2. `RepoSnapshot` loader + unit tests (no UI).
3. Static UI rendering one snapshot; keybinds for focus/scroll/quit.
4. Background loader thread + channel.
5. File watcher with filtering + debounce → live refresh.
6. Branch graph (Option A) + parser tests.
7. zsh cwd hook + repo switching.
8. Edge states (unborn, detached, merge/rebase, not-a-repo).
9. Read-only verification test, acceptance checklist, README, DECISIONS.md.
