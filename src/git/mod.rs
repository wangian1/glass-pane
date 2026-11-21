pub mod diff;
pub mod graph;
pub mod inspect;
pub mod log;
pub mod refs;
pub mod state;
pub mod status;

use crate::config::Config;
use graph::{GitCliGraph, GraphProvider};
use std::{
    path::{Path, PathBuf},
    time::{Instant, SystemTime},
};

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("Not a git repository: {}", .0.display())]
    NotRepository(PathBuf),
    #[error("Cannot load repository: {0}")]
    Git(#[from] git2::Error),
}

#[derive(Debug)]
pub struct RepoSnapshot {
    pub root: PathBuf,
    pub git_dir: PathBuf,
    pub common_dir: PathBuf,
    pub head: String,
    pub head_commit: Option<log::CommitInfo>,
    pub upstream: Option<String>,
    pub state: &'static str,
    pub local_branches: Vec<refs::BranchInfo>,
    pub remote_branches: Vec<refs::BranchInfo>,
    pub last_fetch: Option<SystemTime>,
    pub commits: Vec<log::CommitInfo>,
    pub graph: Result<Vec<graph::GraphLine>, String>,
    pub staging: status::Staging,
    pub loaded_at: Instant,
}

pub fn load(path: &Path, config: &Config) -> Result<RepoSnapshot, LoadError> {
    let repo = git2::Repository::discover(path).map_err(|e| {
        if e.code() == git2::ErrorCode::NotFound {
            LoadError::NotRepository(path.to_path_buf())
        } else {
            LoadError::Git(e)
        }
    })?;
    let root = repo.workdir().unwrap_or(repo.path()).to_path_buf();
    let (head, head_commit) = head(&repo)?;
    let upstream = repo
        .find_branch(&head, git2::BranchType::Local)
        .ok()
        .and_then(|b| b.upstream().ok())
        .and_then(|b| b.name().ok().flatten().map(str::to_string));
    let (local_branches, remote_branches) =
        refs::load(&repo, config.branch_sort, config.show_remote_head)?;
    let common_dir = common_dir(&repo);
    let last_fetch = [
        repo.path().join("FETCH_HEAD"),
        common_dir.join("FETCH_HEAD"),
    ]
    .iter()
    .filter_map(|p| p.metadata().ok()?.modified().ok())
    .max();
    Ok(RepoSnapshot {
        root: root.clone(),
        git_dir: repo.path().to_path_buf(),
        common_dir,
        head,
        head_commit,
        upstream,
        state: state::load(&repo),
        local_branches,
        remote_branches,
        last_fetch,
        commits: log::load(&repo, config.commit_limit)?,
        graph: GitCliGraph.load(&root, config.graph_limit),
        staging: status::load(&repo)?,
        loaded_at: Instant::now(),
    })
}

// Linked worktrees keep per-worktree HEAD/index separately from shared refs.
pub fn common_dir(repo: &git2::Repository) -> PathBuf {
    repo.commondir().to_path_buf()
}

/// Shared HEAD reader for the full dashboard and lightweight shell summary.
pub fn head(repo: &git2::Repository) -> Result<(String, Option<log::CommitInfo>), git2::Error> {
    let head_ref = match repo.head() {
        Ok(head) => Some(head),
        Err(e)
            if matches!(
                e.code(),
                git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
            ) =>
        {
            None
        }
        Err(e) => return Err(e),
    };
    let head_commit = head_ref
        .as_ref()
        .and_then(|r| r.peel_to_commit().ok())
        .map(|c| log::info(&c));
    let detached = repo.head_detached()?;
    let head = if detached {
        format!(
            "DETACHED @ {}",
            head_commit.as_ref().map(|c| c.hash.as_str()).unwrap_or("?")
        )
    } else {
        head_ref
            .as_ref()
            .and_then(|r| r.shorthand().ok())
            .map(str::to_string)
            .or_else(|| {
                repo.find_reference("HEAD").ok().and_then(|r| {
                    r.symbolic_target()
                        .ok()
                        .flatten()
                        .map(|s| s.trim_start_matches("refs/heads/").to_string())
                })
            })
            .unwrap_or_else(|| "Unborn branch".to_string())
    };
    Ok((head, head_commit))
}
