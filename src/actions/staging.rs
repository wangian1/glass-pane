//! Stage or unstage exactly one selected status entry, never working-tree files.
use super::git_command;
use crate::git::{
    self,
    diff::{Kind, Target},
};
use anyhow::{Context, Result, bail};
use git2::Repository;
use std::{
    path::{Component, PathBuf},
    process::Stdio,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Stage,
    Unstage,
}
impl Operation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stage => "Staged",
            Self::Unstage => "Unstaged",
        }
    }
}
#[derive(Debug)]
pub struct Request {
    pub root: PathBuf,
    pub generation: u64,
    pub target: Target,
    pub operation: Operation,
    pub expected_head: (String, Option<git2::Oid>),
}

pub fn execute(request: &Request) -> Result<String> {
    let repo = Repository::open(&request.root)?;
    if repo.is_bare() {
        bail!("Staging requires a working tree.");
    }
    if matches!(request.target.kind, Kind::Conflicted | Kind::Commit(_)) {
        bail!("Resolve the conflict and stage it explicitly in shell mode.");
    }
    match (request.operation, request.target.kind) {
        (Operation::Stage, Kind::Staged) => {
            bail!("Already staged. Select the UNSTAGED entry to include further edits.")
        }
        (Operation::Unstage, kind) if kind != Kind::Staged => {
            bail!("Select a STAGED entry to unstage.")
        }
        _ => {}
    }
    if request.target.paths.is_empty()
        || request.target.paths.iter().any(|p| {
            p.as_os_str().is_empty() || p.components().any(|c| !matches!(c, Component::Normal(_)))
        })
    {
        bail!("Invalid repository-relative file path.");
    }
    let (branch, head) = git::head(&repo)?;
    if (branch, head.map(|c| c.oid)) != request.expected_head {
        bail!("HEAD or branch changed. Refresh and select the file again.");
    }
    let staging = git::status::load(&repo)?;
    let files = match request.target.kind {
        Kind::Staged => &staging.staged,
        Kind::Unstaged => &staging.unstaged,
        Kind::Untracked => &staging.untracked,
        Kind::Conflicted | Kind::Commit(_) => unreachable!(),
    };
    if !files
        .iter()
        .any(|f| f.paths == request.target.paths && f.path == request.target.path)
    {
        bail!("File status changed. Refresh and select the file again.");
    }
    let mut command = git_command(&request.root);
    command.arg("--literal-pathspecs");
    match request.operation {
        Operation::Stage => {
            command.args(["add", "--all"]);
        }
        Operation::Unstage => {
            match repo.head() {
                Ok(_) => {
                    command.args(["restore", "--staged", "--source=HEAD"]);
                }
                Err(e) if e.code() == git2::ErrorCode::UnbornBranch => {
                    // Removing from an unborn index only: -f permits partially
                    // staged new files while --cached guarantees file retention.
                    command.args(["rm", "--cached", "--force", "--ignore-unmatch"]);
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
    let output = command
        .arg("--")
        .args(&request.target.paths)
        .stdin(Stdio::null())
        .output()
        .context("Cannot run Git staging action")?;
    if !output.status.success() {
        bail!(
            "Git refused staging action: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(format!(
        "{} {}",
        request.operation.label(),
        request.target.path
    ))
}
