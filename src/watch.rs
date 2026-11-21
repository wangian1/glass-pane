use crate::{
    cwd,
    event::{Event, WatchCommand},
};
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchTarget {
    pub root: PathBuf,
    pub git_dir: PathBuf,
    pub common_dir: PathBuf,
}

pub fn relevant(path: &Path, target: &WatchTarget, repo: Option<&git2::Repository>) -> bool {
    if path
        .file_name()
        .is_some_and(|s| s.to_string_lossy().ends_with(".lock"))
    {
        return false;
    }
    for git_dir in [&target.git_dir, &target.common_dir] {
        if let Ok(relative) = path.strip_prefix(git_dir) {
            return !relative.starts_with("objects");
        }
    }
    if let Ok(relative) = path.strip_prefix(&target.root) {
        if relative.starts_with(".git/objects") {
            return false;
        }
        if relative.starts_with(".git") || relative.as_os_str().is_empty() {
            return true;
        }
        return !repo.is_some_and(|r| r.is_path_ignored(relative).unwrap_or(false));
    }
    false
}

fn existing_parent(path: &Path) -> Option<PathBuf> {
    path.parent()?
        .ancestors()
        .find(|p| p.is_dir())
        .map(Path::to_path_buf)
}

struct ActiveWatch {
    _watcher: Debouncer<RecommendedWatcher>,
    events: Receiver<DebounceEventResult>,
    cwd_parent: Option<PathBuf>,
    overflow: Arc<AtomicBool>,
}

fn cwd_event(path: &Path, cwd_file: Option<&Path>) -> bool {
    cwd_file.is_some_and(|cwd| path == cwd || cwd.starts_with(path))
}

fn install(
    target: Option<&WatchTarget>,
    cwd_file: Option<&Path>,
    debounce: Duration,
) -> notify::Result<ActiveWatch> {
    // Filter before the bounded callback queue so ignored build output cannot
    // overflow it and trigger unnecessary snapshot loads.
    let (tx, rx) = mpsc::sync_channel(256);
    let overflow = Arc::new(AtomicBool::new(false));
    let callback_overflow = overflow.clone();
    let callback_target = target.cloned();
    let callback_repo = target.and_then(|t| git2::Repository::open(&t.git_dir).ok());
    let callback_cwd = cwd_file.map(Path::to_path_buf);
    let mut watcher = new_debouncer(debounce, move |mut result: DebounceEventResult| {
        if let Ok(events) = &mut result {
            events.retain(|event| {
                cwd_event(&event.path, callback_cwd.as_deref())
                    || callback_target
                        .as_ref()
                        .is_some_and(|t| relevant(&event.path, t, callback_repo.as_ref()))
            });
            if events.is_empty() {
                return;
            }
        }
        if tx.try_send(result).is_err() {
            callback_overflow.store(true, Ordering::Relaxed);
        }
    })?;
    if let Some(t) = target {
        watcher.watcher().watch(&t.root, RecursiveMode::Recursive)?;
        // Linked worktrees/submodules can keep their metadata outside the root.
        if !t.common_dir.starts_with(&t.root) {
            watcher
                .watcher()
                .watch(&t.common_dir, RecursiveMode::Recursive)?;
        }
        if !t.git_dir.starts_with(&t.root) && !t.git_dir.starts_with(&t.common_dir) {
            watcher
                .watcher()
                .watch(&t.git_dir, RecursiveMode::Recursive)?;
        }
    }
    // Watch the parent, not the state file: this also catches atomic replacements.
    // If the directory is absent, move the watch down as ancestors are created.
    let cwd_parent = cwd_file.and_then(existing_parent);
    if let Some(p) = &cwd_parent {
        watcher.watcher().watch(p, RecursiveMode::NonRecursive)?;
    }
    Ok(ActiveWatch {
        _watcher: watcher,
        events: rx,
        cwd_parent,
        overflow,
    })
}

pub fn spawn(
    events: Sender<Event>,
    commands: Receiver<WatchCommand>,
    cwd_file: Option<PathBuf>,
    debounce: Duration,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut target: Option<WatchTarget> = None;
        let mut active: Option<ActiveWatch> = None;
        let mut rebuild = true;
        let mut last_cwd = cwd_file.as_deref().and_then(cwd::read);
        let mut last_poll = Instant::now();
        loop {
            match commands.recv_timeout(Duration::from_millis(50)) {
                Ok(WatchCommand::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(WatchCommand::Target(next)) => {
                    if next != target {
                        target = next;
                        rebuild = true;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            if rebuild {
                active = match install(target.as_ref(), cwd_file.as_deref(), debounce) {
                    Ok(watch) => {
                        let _ = events.send(Event::WatchStatus {
                            polling: false,
                            error: None,
                        });
                        Some(watch)
                    }
                    Err(e) => {
                        let _ = events.send(Event::WatchStatus {
                            polling: true,
                            error: Some(e.to_string()),
                        });
                        None
                    }
                };
                rebuild = false;
            }
            let mut check_cwd = false;
            let mut dirty = false;
            let mut failed = None;
            if let Some(watch) = &active {
                if watch.overflow.swap(false, Ordering::Relaxed) {
                    dirty = true;
                    check_cwd = true;
                }
                for event in watch.events.try_iter() {
                    match event {
                        Ok(paths) => {
                            if paths
                                .iter()
                                .any(|e| cwd_event(&e.path, cwd_file.as_deref()))
                            {
                                check_cwd = true;
                            }
                            // Callback already removed ignored and irrelevant paths.
                            if target.is_some()
                                && paths
                                    .iter()
                                    .any(|e| !cwd_event(&e.path, cwd_file.as_deref()))
                            {
                                dirty = true;
                            }
                        }
                        Err(e) => failed = Some(e.to_string()),
                    }
                }
            }
            if let Some(error) = failed {
                active = None;
                let _ = events.send(Event::WatchStatus {
                    polling: true,
                    error: Some(error),
                });
            }
            if last_poll.elapsed() >= Duration::from_secs(2) {
                check_cwd = true;
                if active.is_none() {
                    dirty = true;
                }
                if let Some(watch) = &active {
                    if cwd_file.as_deref().and_then(existing_parent) != watch.cwd_parent {
                        rebuild = true;
                    }
                    if target
                        .as_ref()
                        .is_some_and(|t| !t.root.is_dir() || !t.git_dir.is_dir())
                    {
                        rebuild = true;
                    }
                }
                last_poll = Instant::now();
            }
            if check_cwd {
                let next = cwd_file.as_deref().and_then(cwd::read);
                // Ignore transient empty reads during truncate-and-write hooks.
                let removed = cwd_file.as_ref().is_some_and(|p| !p.exists());
                if next != last_cwd && (next.is_some() || removed) {
                    let _ = events.send(Event::CwdChanged(next.clone()));
                    last_cwd = next;
                }
            }
            if dirty {
                let _ = events.send(Event::RepoChanged);
            }
        }
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filters_metadata_noise_but_keeps_refs_and_files() {
        let t = WatchTarget {
            root: "/repo".into(),
            git_dir: "/repo/.git".into(),
            common_dir: "/repo/.git".into(),
        };
        for path in [
            "/repo/.git/objects/ab/cdef",
            "/repo/.git/index.lock",
            "/repo/file.lock",
            "/elsewhere/file",
        ] {
            assert!(!relevant(Path::new(path), &t, None), "{path}");
        }
        for path in [
            "/repo/.git/HEAD",
            "/repo/.git/refs/heads/main",
            "/repo/src/main.rs",
        ] {
            assert!(relevant(Path::new(path), &t, None), "{path}");
        }
    }
    #[test]
    fn ignores_gitignored_worktree_files() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        std::fs::write(dir.path().join(".gitignore"), "build/\n").unwrap();
        let t = WatchTarget {
            root: dir.path().into(),
            git_dir: repo.path().into(),
            common_dir: repo.path().into(),
        };
        assert!(!relevant(&dir.path().join("build/noise"), &t, Some(&repo)));
        assert!(relevant(&dir.path().join("README.md"), &t, Some(&repo)));
    }
}
