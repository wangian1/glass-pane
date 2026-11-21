use crate::{
    git::{LoadError, RepoSnapshot},
    watch::WatchTarget,
};
use std::path::PathBuf;

#[derive(Debug)]
pub enum Event {
    InspectionLoaded {
        id: u64,
        result: Result<crate::git::inspect::Inspection, String>,
    },
    CommitPrepared {
        generation: u64,
        result: Box<Result<crate::actions::commit::Plan, String>>,
    },
    StagingChanged {
        generation: u64,
        result: Result<String, String>,
    },
    DiffLoaded {
        id: u64,
        result: Result<crate::git::diff::Preview, String>,
    },
    AmendPrepared {
        generation: u64,
        result: Box<Result<crate::actions::amend::Plan, String>>,
    },
    BranchSwitched {
        generation: u64,
        result: Result<String, String>,
    },
    Loaded {
        generation: u64,
        result: Box<Result<RepoSnapshot, LoadError>>,
    },
    RepoChanged,
    CwdChanged(Option<PathBuf>),
    WatchStatus {
        polling: bool,
        error: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshRequest {
    pub path: PathBuf,
    pub generation: u64,
}

pub enum WatchCommand {
    Target(Option<WatchTarget>),
    Stop,
}
