use crate::{
    actions::{BranchTarget, SwitchRequest},
    config::Config,
    event::{Event, RefreshRequest, WatchCommand},
    git,
    watch::WatchTarget,
};
use std::{
    path::PathBuf,
    sync::{Arc, Condvar, Mutex, mpsc::Sender},
    thread,
};

type Mailbox = Arc<(Mutex<(bool, Option<RefreshRequest>)>, Condvar)>;

pub struct Loader {
    mailbox: Mailbox,
    handle: Option<thread::JoinHandle<()>>,
}

impl Loader {
    pub fn spawn(config: Config, events: Sender<Event>, watches: Sender<WatchCommand>) -> Self {
        let mailbox: Mailbox = Arc::new((Mutex::new((false, None)), Condvar::new()));
        let worker_mailbox = mailbox.clone();
        let handle = thread::spawn(move || {
            loop {
                let request = {
                    let (mutex, wake) = &*worker_mailbox;
                    let mut pending = mutex.lock().unwrap();
                    while !pending.0 && pending.1.is_none() {
                        pending = wake.wait(pending).unwrap();
                    }
                    if pending.0 {
                        break;
                    }
                    pending.1.take().unwrap()
                };
                let result = git::load(&request.path, &config);
                // Do not publish watches/snapshots for a superseded directory.
                let pending = worker_mailbox.0.lock().unwrap();
                if pending.0 {
                    break;
                }
                if pending
                    .1
                    .as_ref()
                    .is_some_and(|next| next.generation != request.generation)
                {
                    continue;
                }
                let target = result.as_ref().ok().map(|s| WatchTarget {
                    root: s.root.clone(),
                    git_dir: s.git_dir.clone(),
                    common_dir: s.common_dir.clone(),
                });
                let _ = watches.send(WatchCommand::Target(target));
                if events
                    .send(Event::Loaded {
                        generation: request.generation,
                        result: Box::new(result),
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            mailbox,
            handle: Some(handle),
        }
    }

    pub fn is_finished(&self) -> bool {
        self.handle
            .as_ref()
            .is_some_and(thread::JoinHandle::is_finished)
    }

    pub fn request(&self, path: PathBuf, generation: u64) {
        let mut pending = self.mailbox.0.lock().unwrap();
        pending.1 = Some(RefreshRequest { path, generation });
        self.mailbox.1.notify_one();
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        self.mailbox.0.lock().unwrap().0 = true;
        self.mailbox.1.notify_one();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub struct App {
    pub selected_commit: Option<git2::Oid>,
    pub inspection: Option<git::inspect::View>,
    pub inspection_serial: u64,
    pub selected_file: Option<git::diff::Target>,
    pub diff: Option<git::diff::View>,
    pub diff_serial: u64,
    pub cwd: PathBuf,
    pub launch_dir: PathBuf,
    pub following: bool,
    pub generation: u64,
    pub snapshot: Option<git::RepoSnapshot>,
    pub snapshot_generation: Option<u64>,
    pub selected_branch: Option<BranchTarget>,
    pub switching: bool,
    pub staging_busy: bool,
    pub commit_preparing: bool,
    pub commit: Option<crate::actions::commit::Review>,
    pub amend_preparing: bool,
    pub amend: Option<crate::actions::amend::Review>,
    pub action_message: Option<String>,
    pub action_error: Option<String>,
    pub error: Option<String>,
    pub focus: usize,
    pub scroll: [usize; 4],
    pub max_scroll: [usize; 4],
    pub help: bool,
    pub shell_error: Option<String>,
    pub maximized: bool,
    pub polling: bool,
    pub watcher_error: Option<String>,
    pub loading: bool,
    pub no_color: bool,
}

impl App {
    pub fn new(path: PathBuf) -> Self {
        Self {
            selected_commit: None,
            inspection: None,
            inspection_serial: 0,
            selected_file: None,
            diff: None,
            diff_serial: 0,
            cwd: path.clone(),
            launch_dir: path,
            following: false,
            generation: 0,
            snapshot: None,
            snapshot_generation: None,
            selected_branch: None,
            switching: false,
            staging_busy: false,
            commit_preparing: false,
            commit: None,
            amend_preparing: false,
            amend: None,
            action_message: None,
            action_error: None,
            error: None,
            focus: 0,
            scroll: [0; 4],
            max_scroll: [0; 4],
            help: false,
            shell_error: None,
            maximized: false,
            polling: false,
            watcher_error: None,
            loading: true,
            no_color: std::env::var_os("NO_COLOR").is_some(),
        }
    }
    pub fn apply(&mut self, generation: u64, result: Result<git::RepoSnapshot, git::LoadError>) {
        if generation != self.generation {
            return;
        }
        self.loading = false;
        match result {
            Ok(snapshot) => {
                if self
                    .snapshot
                    .as_ref()
                    .is_some_and(|s| s.root != snapshot.root)
                {
                    self.scroll = [0; 4];
                    self.selected_branch = None;
                    self.selected_file = None;
                    self.diff = None;
                    self.inspection = None;
                    self.commit = None;
                    self.selected_commit = None;
                }
                self.snapshot = Some(snapshot);
                self.snapshot_generation = Some(generation);
                let choices = self.branch_choices();
                if !self
                    .selected_branch
                    .as_ref()
                    .is_some_and(|selected| choices.contains(selected))
                {
                    self.selected_branch = choices.into_iter().next();
                }
                let files = self.file_choices();
                if !self
                    .selected_file
                    .as_ref()
                    .is_some_and(|f| files.contains(f))
                {
                    self.selected_file = self
                        .selected_file
                        .as_ref()
                        .and_then(|old| {
                            files
                                .iter()
                                .find(|f| {
                                    f.path == old.path
                                        || f.paths.iter().any(|path| old.paths.contains(path))
                                })
                                .cloned()
                        })
                        .or_else(|| files.into_iter().next());
                }
                let commits = &self.snapshot.as_ref().unwrap().commits;
                if !self
                    .selected_commit
                    .is_some_and(|oid| commits.iter().any(|c| c.oid == oid))
                {
                    self.selected_commit = commits.first().map(|c| c.oid);
                }
                self.error = None;
            }
            Err(e) => {
                self.diff = None;
                self.inspection = None;
                self.commit = None;
                self.selected_commit = None;
                self.selected_file = None;
                self.snapshot = None;
                self.snapshot_generation = None;
                self.selected_branch = None;
                self.error = Some(e.to_string());
            }
        }
    }
    pub fn change_cwd(&mut self, path: Option<PathBuf>) {
        self.action_message = None;
        self.action_error = None;
        self.commit = None;
        self.amend = None;
        self.diff = None;
        self.inspection = None;
        self.following = path.is_some();
        self.cwd = path.unwrap_or_else(|| self.launch_dir.clone());
        self.generation = self.generation.wrapping_add(1);
        self.loading = true;
    }
    pub fn scroll_by(&mut self, down: bool) {
        if self.focus == 2 {
            if let Some(snapshot) = &self.snapshot {
                let i = self
                    .selected_commit
                    .and_then(|oid| snapshot.commits.iter().position(|c| c.oid == oid))
                    .unwrap_or(0);
                let next = if down {
                    i.saturating_add(1)
                        .min(snapshot.commits.len().saturating_sub(1))
                } else {
                    i.saturating_sub(1)
                };
                self.selected_commit = snapshot.commits.get(next).map(|c| c.oid);
            }
            return;
        }
        if self.focus == 3 {
            let choices = self.file_choices();
            let index = self
                .selected_file
                .as_ref()
                .and_then(|f| choices.iter().position(|x| x == f))
                .unwrap_or(0);
            let next = if down {
                index.saturating_add(1).min(choices.len().saturating_sub(1))
            } else {
                index.saturating_sub(1)
            };
            self.selected_file = choices.get(next).cloned();
            return;
        }
        if self.focus == 1 {
            let choices = self.branch_choices();
            let index = self
                .selected_branch
                .as_ref()
                .and_then(|selected| choices.iter().position(|b| b == selected))
                .unwrap_or(0);
            let next = if down {
                index.saturating_add(1).min(choices.len().saturating_sub(1))
            } else {
                index.saturating_sub(1)
            };
            self.selected_branch = choices.get(next).cloned();
            return;
        }
        let i = self.focus;
        self.scroll[i] = if down {
            self.scroll[i].saturating_add(1).min(self.max_scroll[i])
        } else {
            self.scroll[i].saturating_sub(1)
        };
    }
    pub fn branch_choices(&self) -> Vec<BranchTarget> {
        let Some(snapshot) = &self.snapshot else {
            return vec![];
        };
        snapshot
            .local_branches
            .iter()
            .map(|b| BranchTarget {
                name: b.name.clone(),
                remote: false,
            })
            .chain(snapshot.remote_branches.iter().map(|b| BranchTarget {
                name: b.name.clone(),
                remote: true,
            }))
            .collect()
    }
    pub fn selected_branch_row(&self) -> Option<usize> {
        let snapshot = self.snapshot.as_ref()?;
        let selected = self.selected_branch.as_ref()?;
        if selected.remote {
            snapshot
                .remote_branches
                .iter()
                .position(|b| b.name == selected.name)
                .map(|index| 2 + snapshot.local_branches.len().max(1) + index)
        } else {
            snapshot
                .local_branches
                .iter()
                .position(|b| b.name == selected.name)
                .map(|i| 1 + i)
        }
    }
    pub fn go_edge(&mut self, bottom: bool) {
        if self.focus == 2 {
            self.selected_commit = self
                .snapshot
                .as_ref()
                .and_then(|s| {
                    if bottom {
                        s.commits.last()
                    } else {
                        s.commits.first()
                    }
                })
                .map(|c| c.oid);
            return;
        }
        if self.focus == 3 {
            let choices = self.file_choices();
            self.selected_file = if bottom {
                choices.last()
            } else {
                choices.first()
            }
            .cloned();
            return;
        }
        if self.focus == 1 {
            let choices = self.branch_choices();
            if !bottom {
                self.scroll[1] = 0;
            }
            self.selected_branch = if bottom {
                choices.last()
            } else {
                choices.first()
            }
            .cloned();
        } else {
            self.scroll[self.focus] = if bottom {
                self.max_scroll[self.focus]
            } else {
                0
            };
        }
    }
    pub fn file_groups(&self) -> Vec<(git::diff::Kind, &Vec<git::status::FileStatus>)> {
        use git::diff::Kind;
        let Some(s) = &self.snapshot else {
            return vec![];
        };
        vec![
            (Kind::Conflicted, &s.staging.conflicted),
            (Kind::Staged, &s.staging.staged),
            (Kind::Unstaged, &s.staging.unstaged),
            (Kind::Untracked, &s.staging.untracked),
        ]
    }
    pub fn file_choices(&self) -> Vec<git::diff::Target> {
        self.file_groups()
            .into_iter()
            .flat_map(|(kind, files)| {
                files.iter().map(move |f| git::diff::Target {
                    kind,
                    path: f.path.clone(),
                    paths: f.paths.clone(),
                })
            })
            .collect()
    }
    pub fn selected_file_row(&self) -> Option<usize> {
        let selected = self.selected_file.as_ref()?;
        let mut row = 0;
        for (kind, files) in self.file_groups() {
            if files.is_empty() {
                continue;
            }
            row += 1;
            for file in files {
                if kind == selected.kind
                    && file.path == selected.path
                    && file.paths == selected.paths
                {
                    return Some(row);
                }
                row += 1;
            }
        }
        None
    }
    pub fn open_diff(&mut self, worker: &git::diff::Worker) {
        if self.commit_preparing
            || self.amend_preparing
            || self.commit.is_some()
            || self.amend.is_some()
        {
            return;
        }
        if self.snapshot_generation != Some(self.generation) {
            return;
        }
        let Some(target) = self.selected_file.clone() else {
            return;
        };
        self.diff_serial = self.diff_serial.wrapping_add(1);
        let id = self.diff_serial;
        let root = self.snapshot.as_ref().unwrap().root.clone();
        worker.request(git::diff::Request {
            root,
            target: target.clone(),
            id,
        });
        self.diff = Some(git::diff::View {
            id,
            target,
            result: None,
            scroll: 0,
            horizontal: 0,
            max_scroll: 0,
        });
    }
    pub fn apply_diff(&mut self, id: u64, result: Result<git::diff::Preview, String>) {
        if let Some(view) = &mut self.diff
            && view.id == id
        {
            view.result = Some(result);
        }
    }
    pub fn selected_commit_row(&self) -> Option<usize> {
        let oid = self.selected_commit?;
        self.snapshot
            .as_ref()?
            .commits
            .iter()
            .position(|c| c.oid == oid)
    }
    pub fn open_inspection(&mut self, worker: &git::diff::Worker) {
        if self.commit_preparing
            || self.amend_preparing
            || self.commit.is_some()
            || self.amend.is_some()
        {
            return;
        }
        if self.snapshot_generation != Some(self.generation) {
            return;
        }
        let (Some(oid), Some(snapshot)) = (self.selected_commit, &self.snapshot) else {
            return;
        };
        self.inspection_serial = self.inspection_serial.wrapping_add(1);
        worker.inspect(self.inspection_serial, snapshot.root.clone(), oid);
        self.inspection = Some(git::inspect::View::new(self.inspection_serial, oid));
    }
    pub fn apply_inspection(&mut self, id: u64, result: Result<git::inspect::Inspection, String>) {
        if let Some(view) = &mut self.inspection
            && view.id == id
        {
            view.result = Some(result);
        }
    }
    pub fn open_commit_diff(&mut self, worker: &git::diff::Worker) {
        let Some(target) = self.inspection.as_ref().and_then(|v| v.file()).cloned() else {
            return;
        };
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        self.diff_serial = self.diff_serial.wrapping_add(1);
        worker.request(git::diff::Request {
            root: snapshot.root.clone(),
            target: target.clone(),
            id: self.diff_serial,
        });
        self.diff = Some(git::diff::View {
            id: self.diff_serial,
            target,
            result: None,
            scroll: 0,
            horizontal: 0,
            max_scroll: 0,
        });
    }
    pub fn refresh_diff(&mut self, worker: &git::diff::Worker) {
        if let (Some(view), Some(snapshot)) = (&mut self.diff, &self.snapshot) {
            if matches!(view.target.kind, git::diff::Kind::Commit(_)) {
                return;
            }
            self.diff_serial = self.diff_serial.wrapping_add(1);
            view.id = self.diff_serial;
            worker.request(git::diff::Request {
                root: snapshot.root.clone(),
                target: view.target.clone(),
                id: view.id,
            });
        }
    }
    pub fn staging_request(
        &self,
        operation: crate::actions::staging::Operation,
    ) -> Option<crate::actions::staging::Request> {
        if self.staging_busy
            || self.switching
            || self.amend_preparing
            || self.commit_preparing
            || self.commit.is_some()
            || self.amend.is_some()
            || self.loading
            || self.snapshot_generation != Some(self.generation)
        {
            return None;
        }
        Some(crate::actions::staging::Request {
            root: self.snapshot.as_ref()?.root.clone(),
            generation: self.generation,
            target: self.selected_file.clone()?,
            operation,
            expected_head: (
                self.snapshot.as_ref()?.head.clone(),
                self.snapshot.as_ref()?.head_commit.as_ref().map(|c| c.oid),
            ),
        })
    }
    pub fn switch_request(&self) -> Option<SwitchRequest> {
        if self.staging_busy
            || self.switching
            || self.amend_preparing
            || self.commit_preparing
            || self.commit.is_some()
            || self.amend.is_some()
            || self.snapshot_generation != Some(self.generation)
        {
            return None;
        }
        Some(SwitchRequest {
            root: self.snapshot.as_ref()?.root.clone(),
            generation: self.generation,
            target: self.selected_branch.clone()?,
        })
    }
}
