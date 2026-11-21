//! Review and execute a native Git amend without staging files automatically.
use crate::git::{self, log::CommitInfo, status::FileStatus};
use anyhow::{Context, Result, bail};
use git2::{Oid, Repository, RepositoryState};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    StagedKeepMessage,
    MessageOnly,
    StagedEditMessage,
}
impl Mode {
    pub const ALL: [Self; 3] = [
        Self::StagedKeepMessage,
        Self::MessageOnly,
        Self::StagedEditMessage,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::StagedKeepMessage => "Add staged changes; keep message",
            Self::MessageOnly => "Edit message only; exclude staged changes",
            Self::StagedEditMessage => "Add staged changes and edit message",
        }
    }
    pub fn includes_staged(self) -> bool {
        self != Self::MessageOnly
    }
}

#[derive(Debug, PartialEq, Eq)]
struct IndexEntryState {
    path: Vec<u8>,
    id: Oid,
    mode: u32,
    flags: u16,
    extended: u16,
}

#[derive(Debug)]
pub struct Plan {
    pub root: PathBuf,
    pub branch: String,
    pub commit: CommitInfo,
    pub staged: Vec<FileStatus>,
    pub excluded_count: usize,
    head: Oid,
    head_ref: Vec<u8>,
    index: Vec<IndexEntryState>,
}

fn index_bytes(repo: &Repository) -> io::Result<Option<Vec<u8>>> {
    match fs::read(repo.path().join("index")) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn prepare(root: &Path) -> Result<Plan> {
    let repo = Repository::open(root).context("Cannot open repository for amendment")?;
    if repo.is_bare() {
        bail!("Amend requires a working tree.");
    }
    if repo.state() != RepositoryState::Clean {
        bail!(
            "Finish the current merge, rebase, cherry-pick or other Git operation before amending."
        );
    }
    let before = index_bytes(&repo)?;
    let index = repo
        .index()?
        .iter()
        .map(|entry| IndexEntryState {
            path: entry.path,
            id: entry.id,
            mode: entry.mode,
            flags: entry.flags,
            extended: entry.flags_extended,
        })
        .collect();
    let head = repo.head().context("There is no commit to amend yet")?;
    let commit = head
        .peel_to_commit()
        .context("There is no commit to amend yet")?;
    let staging = git::status::load(&repo)?;
    if !staging.conflicted.is_empty() {
        bail!("Resolve all conflicts before amending.");
    }
    let (branch, _) = git::head(&repo)?;
    if index_bytes(&repo)? != before || repo.head()?.peel_to_commit()?.id() != commit.id() {
        bail!("Repository changed while preparing review. Open amend again.");
    }
    Ok(Plan {
        root: repo
            .workdir()
            .context("Amend requires a working tree")?
            .into(),
        branch,
        commit: git::log::info(&commit),
        staged: staging.staged,
        excluded_count: staging.unstaged.len() + staging.untracked.len(),
        head: commit.id(),
        head_ref: head.name_bytes().to_vec(),
        index,
    })
}

pub fn validate(plan: &Plan, mode: Mode) -> Result<()> {
    let current = prepare(&plan.root)?;
    if current.head != plan.head || current.head_ref != plan.head_ref {
        bail!("HEAD or the active branch changed after review. Open amend again.");
    }
    if current.index != plan.index {
        bail!("Staged contents changed after review. Open amend again to review the new index.");
    }
    if mode.includes_staged() && plan.staged.is_empty() {
        bail!("No staged changes. Stage files first or choose message-only amendment.");
    }
    Ok(())
}

/// Revalidate immediately before constructing the command. The caller chooses
/// terminal I/O; the application inherits it for editors, hooks and signing.
pub fn command(plan: &Plan, mode: Mode) -> Result<Command> {
    validate(plan, mode)?;
    let mut command = super::git_command(&plan.root);
    command.args(["commit", "--amend"]);
    match mode {
        Mode::StagedKeepMessage => {
            command.arg("--no-edit");
        }
        Mode::MessageOnly => {
            command.args(["--only", "--edit"]);
        }
        Mode::StagedEditMessage => {
            command.arg("--edit");
        }
    }
    crate::shell::reset_child_interrupt(&mut command);
    Ok(command)
}

pub fn finish(plan: &Plan, status: ExitStatus) -> Result<String> {
    let repo = Repository::open(&plan.root)?;
    let new = repo.head()?.peel_to_commit()?.id();
    let old = &plan.commit.hash;
    let short = new.to_string()[..7].to_string();
    if !status.success() {
        if new != plan.head {
            return Ok(format!(
                "Git reported {status}; HEAD changed {old} → {short}. Review the current commit; nothing was rolled back."
            ));
        }
        bail!(
            "Amend did not complete ({status}); commit {old} is unchanged. See Git/editor output in the normal terminal."
        );
    }
    Ok(if new == plan.head {
        format!("Amend completed; commit remains {old}")
    } else {
        format!("Amended {old} → {short}")
    })
}

pub fn execute(plan: &Plan, mode: Mode) -> Result<String> {
    let status = command(plan, mode)?
        .status()
        .context("Cannot run git commit --amend")?;
    finish(plan, status)
}

#[derive(Debug)]
pub struct Review {
    pub plan: Plan,
    pub mode: Mode,
    pub file_scroll: usize,
}
impl Review {
    pub fn new(plan: Plan) -> Self {
        let mode = if plan.staged.is_empty() {
            Mode::MessageOnly
        } else {
            Mode::StagedKeepMessage
        };
        Self {
            plan,
            mode,
            file_scroll: 0,
        }
    }
    pub fn choose(&mut self, mode: Mode) {
        if !mode.includes_staged() || !self.plan.staged.is_empty() {
            self.mode = mode;
        }
    }
    pub fn scroll_files(&mut self, down: bool, terminal_height: u16) {
        let page = terminal_height
            .saturating_sub(2)
            .min(30)
            .saturating_sub(15)
            .max(1) as usize;
        let max = self.plan.staged.len().saturating_sub(page);
        self.file_scroll = if down {
            self.file_scroll.saturating_add(page).min(max)
        } else {
            self.file_scroll.saturating_sub(page)
        };
    }
    pub fn step(&mut self, down: bool) {
        let current = Mode::ALL.iter().position(|m| *m == self.mode).unwrap_or(0);
        for offset in 1..=3 {
            let index = if down {
                (current + offset) % 3
            } else {
                (current + 3 - offset) % 3
            };
            let mode = Mode::ALL[index];
            if !mode.includes_staged() || !self.plan.staged.is_empty() {
                self.mode = mode;
                break;
            }
        }
    }
}
