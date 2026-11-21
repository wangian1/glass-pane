# Glasspane user guide

See the [README](../README.md) for installation and shortcuts. Press `?` for help inside Glasspane.

## Stage, review, and commit

1. Use `Tab` to focus **Staging**, then select a file with `↑` / `↓` or `k` / `j`.
2. Press `Enter` to preview the diff. Staged entries compare HEAD with the index. Unstaged entries compare the index with your working tree. Press `Esc` to return.
3. Press `s` to stage the whole file or `u` on a staged entry to unstage it while keeping your edits.
4. Press `c` to review staged files and write a message. `Enter` adds a line, `Ctrl+S` commits, and `Esc` cancels.

Press `a` to amend HEAD. Choose `1` to include staged changes, `2` to edit the message, or `3` to do both. Press `Enter` to confirm. Amending rewrites your latest commit.

## Inspect history and switch branches

- **Recent Commits** — Press `Enter` on a commit to see its message and changed files. Use `Tab` to move to the file list and `Enter` to open a diff.
- **Branches** — Press `Enter` to switch to the selected branch. Selecting a remote-tracking branch sets up a local tracking branch if needed. Git blocks switches that would overwrite local edits.
- **Graph** — Press `Space` to expand the graph. The same key expands any panel.

## Open a shell

Press `!` to open your shell in the directory shown in the header. Use it to fetch, push, or run other Git commands. In zsh, the prompt shows your branch, latest commit, and file counts. Type `exit` or press `Ctrl+D` to return and refresh.

## Shell directory tracking (optional)

Add this to `~/.zshrc` and restart zsh to let Glasspane follow directory changes.

```zsh
eval "$(glass init zsh)"
```

When several shells use the hook, Glasspane follows the most recent directory change. Run `glass --no-follow` to stay in the launch directory.

## Configuration

Optional settings go in `~/.config/glasspane/config.toml` or `$XDG_CONFIG_HOME/glasspane/config.toml`. Run `glass --print-config` to see the effective settings.

```toml
commit_limit = 50
graph_limit = 200
debounce_ms = 150
branch_sort = "date" # or "name"
follow_cwd = true
show_remote_head = false
```

## Limitations and next steps

macOS is the primary platform. Linux remains unverified. Use shell mode for hunk staging and conflict resolution. Remote-tracking branches reflect your last fetch.
