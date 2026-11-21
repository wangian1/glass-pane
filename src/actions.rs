//! Explicit repository writes, separate from the read-only snapshot loaders.
pub mod amend;
pub mod commit;
pub mod staging;
use crate::event::Event;
use anyhow::{Context, Result, bail};
use git2::{BranchType, Repository};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Sender, SyncSender},
    thread,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchTarget {
    pub name: String,
    pub remote: bool,
}

#[derive(Debug)]
pub struct SwitchRequest {
    pub root: PathBuf,
    pub generation: u64,
    pub target: BranchTarget,
}

pub fn switch_branch(root: &Path, target: &BranchTarget) -> Result<String> {
    let repo = Repository::open(root).context("Cannot open repository for branch switch")?;
    let mut create_tracking = false;
    let local_name = if target.remote {
        let remote_branch = repo
            .find_branch(&target.name, BranchType::Remote)
            .context("Remote-tracking branch no longer exists")?;
        let reference = remote_branch.get();
        if reference.symbolic_target_bytes().is_some() {
            bail!("Select a remote branch rather than a symbolic remote HEAD.");
        }
        let remote = repo.branch_remote_name(reference.name()?)?;
        let remote = remote.as_str().context("Remote name is not valid UTF-8")?;
        let name = target
            .name
            .strip_prefix(&format!("{remote}/"))
            .context("Cannot determine the local tracking branch name")?
            .to_string();
        match repo.find_branch(&name, BranchType::Local) {
            Ok(local) => {
                let tracks_selected = local
                    .upstream()
                    .ok()
                    .is_some_and(|upstream| upstream.get().name_bytes() == reference.name_bytes());
                if !tracks_selected {
                    bail!(
                        "Local branch '{name}' already exists and does not track '{}'. Select the local branch directly, or create a differently named tracking branch in shell mode.",
                        target.name
                    );
                }
            }
            Err(e) if e.code() == git2::ErrorCode::NotFound => create_tracking = true,
            Err(e) => return Err(e.into()),
        }
        name
    } else {
        repo.find_branch(&target.name, BranchType::Local)
            .context("Local branch no longer exists")?;
        target.name.clone()
    };
    if repo
        .head()
        .ok()
        .is_some_and(|head| head.name_bytes() == format!("refs/heads/{local_name}").as_bytes())
    {
        return Ok(format!("Already on branch {local_name}"));
    }
    // Git performs its normal safe checkout, operation-state and linked-worktree
    // checks. Never force, stash, discard changes, detach HEAD or contact a remote.
    let mut command = git_command(root);
    command.args(["switch", "--no-guess", "--no-recurse-submodules"]);
    if create_tracking {
        command
            .args(["--track", "--create"])
            .arg(&local_name)
            .arg("--")
            .arg(&target.name);
    } else {
        command.arg("--").arg(&local_name);
    }
    let output = command
        .stdin(Stdio::null())
        .output()
        .context("Cannot run git switch")?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        // A post-checkout hook can fail after Git has successfully moved HEAD.
        // Report the real outcome instead of implying the old branch is active.
        let switched = Repository::open(root).ok().is_some_and(|repo| {
            repo.head().ok().is_some_and(|head| {
                head.name_bytes() == format!("refs/heads/{local_name}").as_bytes()
            })
        });
        if switched {
            return Ok(format!(
                "Switched to {local_name}; Git reported: {}",
                if detail.trim().is_empty() {
                    output.status.to_string()
                } else {
                    detail.trim().replace('\n', " ")
                }
            ));
        }
        bail!(
            "{}",
            if detail.trim().is_empty() {
                "Git refused the branch switch."
            } else {
                detail.trim()
            }
        );
    }
    Ok(if create_tracking {
        format!("Switched to {local_name} (tracking {})", target.name)
    } else {
        format!("Switched to {local_name}")
    })
}

pub(crate) fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_INDEX_FILE",
        "GIT_NAMESPACE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(key);
    }
    command.arg("--no-pager").arg("-C").arg(root);
    command
}

#[derive(Debug)]
enum Job {
    Stage(staging::Request),
    Switch(SwitchRequest),
    PrepareCommit { root: PathBuf, generation: u64 },
    PrepareAmend { root: PathBuf, generation: u64 },
}

pub struct Actions {
    sender: Option<SyncSender<Job>>,
    handle: Option<thread::JoinHandle<()>>,
}

impl Actions {
    pub fn spawn(events: Sender<Event>) -> Self {
        let (sender, requests) = mpsc::sync_channel::<Job>(1);
        let handle = thread::spawn(move || {
            for request in requests {
                let event = match request {
                    Job::Stage(request) => Event::StagingChanged {
                        generation: request.generation,
                        result: staging::execute(&request).map_err(|e| format!("{e:#}")),
                    },
                    Job::Switch(request) => Event::BranchSwitched {
                        generation: request.generation,
                        result: switch_branch(&request.root, &request.target)
                            .map_err(|e| format!("{e:#}")),
                    },
                    Job::PrepareCommit { root, generation } => Event::CommitPrepared {
                        generation,
                        result: Box::new(commit::prepare(&root).map_err(|e| format!("{e:#}"))),
                    },
                    Job::PrepareAmend { root, generation } => Event::AmendPrepared {
                        generation,
                        result: Box::new(amend::prepare(&root).map_err(|e| format!("{e:#}"))),
                    },
                };
                if events.send(event).is_err() {
                    break;
                }
            }
        });
        Self {
            sender: Some(sender),
            handle: Some(handle),
        }
    }
    pub fn stage(&self, request: staging::Request) -> Result<()> {
        self.sender
            .as_ref()
            .context("Action worker has stopped")?
            .try_send(Job::Stage(request))
            .context("Action worker is busy")
    }
    pub fn request(&self, request: SwitchRequest) -> Result<()> {
        self.sender
            .as_ref()
            .context("Branch switch worker has stopped")?
            .try_send(Job::Switch(request))
            .context("Branch switch worker is busy")
    }
    pub fn prepare_commit(&self, root: PathBuf, generation: u64) -> Result<()> {
        self.sender
            .as_ref()
            .context("Action worker has stopped")?
            .try_send(Job::PrepareCommit { root, generation })
            .context("Action worker is busy")
    }
    pub fn prepare_amend(&self, root: PathBuf, generation: u64) -> Result<()> {
        self.sender
            .as_ref()
            .context("Action worker has stopped")?
            .try_send(Job::PrepareAmend { root, generation })
            .context("Action worker is busy")
    }
    pub fn is_finished(&self) -> bool {
        self.handle
            .as_ref()
            .is_some_and(thread::JoinHandle::is_finished)
    }
}

impl Drop for Actions {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
