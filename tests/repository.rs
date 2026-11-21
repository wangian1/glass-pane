use git2::{IndexAddOption, Oid, Repository, Signature};
use glasspane::{
    config::{BranchSort, Config},
    git::{
        self,
        graph::{GitCliGraph, GraphProvider},
    },
    watch::{self, WatchTarget},
};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, SystemTime},
};
use tempfile::TempDir;

struct Fixture {
    dir: TempDir,
    repo: Repository,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        repo.set_head("refs/heads/main").unwrap();
        Self { dir, repo }
    }
    fn write(&self, name: &str, text: &str) {
        fs::write(self.dir.path().join(name), text).unwrap();
    }
    fn stage(&self, name: &str) {
        let mut index = self.repo.index().unwrap();
        index.add_path(Path::new(name)).unwrap();
        index.write().unwrap();
    }
    fn commit(&self, message: &str) -> Oid {
        let mut index = self.repo.index().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = self.repo.find_tree(tree_id).unwrap();
        let signature = Signature::now("Gitpane Test", "test@example.invalid").unwrap();
        let parent = self.repo.head().ok().and_then(|r| r.peel_to_commit().ok());
        let parents: Vec<_> = parent.iter().collect();
        self.repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                message,
                &tree,
                &parents,
            )
            .unwrap()
    }
    fn initial(&self) -> Oid {
        self.write("file.txt", "original\n");
        self.stage("file.txt");
        self.commit("Initial commit")
    }
    fn snapshot(&self) -> git::RepoSnapshot {
        git::load(self.dir.path(), &Config::default()).unwrap()
    }
}

#[test]
fn unborn_repository_and_discovery_from_subdirectory() {
    let f = Fixture::new();
    fs::create_dir(f.dir.path().join("sub")).unwrap();
    let s = git::load(&f.dir.path().join("sub"), &Config::default()).unwrap();
    assert_eq!(s.head, "main");
    assert!(s.commits.is_empty());
    assert!(s.graph.unwrap().is_empty());
    assert!(s.head_commit.is_none());
    assert!(s.local_branches.is_empty());
}

#[test]
fn not_a_repository_is_a_typed_error() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        git::load(dir.path(), &Config::default()),
        Err(git::LoadError::NotRepository(_))
    ));
}

#[test]
fn log_limit_branch_sort_and_detached_head() {
    let f = Fixture::new();
    let first = f.initial();
    f.repo
        .branch("aaa", &f.repo.find_commit(first).unwrap(), false)
        .unwrap();
    f.write("file.txt", "second\n");
    f.stage("file.txt");
    f.commit("Second commit");
    let s = git::load(
        f.dir.path(),
        &Config {
            commit_limit: 1,
            branch_sort: BranchSort::Name,
            ..Config::default()
        },
    )
    .unwrap();
    assert_eq!(s.commits.len(), 1);
    assert_eq!(s.commits[0].subject, "Second commit");
    assert_eq!(s.local_branches[0].name, "main");
    f.repo.set_head_detached(first).unwrap();
    let s = f.snapshot();
    assert!(s.head.starts_with("DETACHED @ "));
    assert_eq!(s.commits[0].subject, "Initial commit");
}

#[test]
fn partial_staging_untracked_and_ignored_files() {
    let f = Fixture::new();
    f.initial();
    f.write("file.txt", "staged\n");
    f.stage("file.txt");
    f.write("file.txt", "unstaged\n");
    f.write("notes.txt", "untracked");
    f.write(".gitignore", "ignored.txt\n");
    f.write("ignored.txt", "ignored");
    let s = f.snapshot().staging;
    assert_eq!(s.staged[0].path, "file.txt");
    assert_eq!(s.unstaged[0].path, "file.txt");
    assert_eq!(s.staged[0].code, 'M');
    assert!(s.untracked.iter().any(|f| f.path == "notes.txt"));
    assert!(!s.untracked.iter().any(|f| f.path == "ignored.txt"));
}

#[test]
fn staged_rename_has_old_and_new_paths() {
    let f = Fixture::new();
    f.initial();
    fs::rename(
        f.dir.path().join("file.txt"),
        f.dir.path().join("renamed.txt"),
    )
    .unwrap();
    let mut index = f.repo.index().unwrap();
    index.add_all(["*"], IndexAddOption::DEFAULT, None).unwrap();
    index.write().unwrap();
    let staging = f.snapshot().staging;
    assert_eq!(staging.staged.len(), 1);
    assert_eq!(staging.staged[0].code, 'R');
    assert_eq!(staging.staged[0].path, "file.txt → renamed.txt");
}

#[test]
fn unstaged_rename_is_detected() {
    let f = Fixture::new();
    f.initial();
    fs::rename(
        f.dir.path().join("file.txt"),
        f.dir.path().join("renamed.txt"),
    )
    .unwrap();
    let staging = f.snapshot().staging;
    assert!(
        staging
            .unstaged
            .iter()
            .any(|f| f.code == 'R' && f.path == "file.txt → renamed.txt")
    );
}

#[test]
fn local_origin_fetch_and_upstream_use_only_local_refs() {
    let f = Fixture::new();
    f.initial();
    let origin_dir = tempfile::tempdir().unwrap();
    let origin = Repository::init_bare(origin_dir.path()).unwrap();
    let mut push = f
        .repo
        .remote_anonymous(origin_dir.path().to_str().unwrap())
        .unwrap();
    push.push(&["refs/heads/main:refs/heads/main"], None)
        .unwrap();
    origin.set_head("refs/heads/main").unwrap();
    let mut remote = f
        .repo
        .remote("origin", origin_dir.path().to_str().unwrap())
        .unwrap();
    remote
        .fetch(&["refs/heads/*:refs/remotes/origin/*"], None, None)
        .unwrap();
    f.repo
        .reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            true,
            "test",
        )
        .unwrap();
    f.repo
        .find_branch("main", git2::BranchType::Local)
        .unwrap()
        .set_upstream(Some("origin/main"))
        .unwrap();
    let s = f.snapshot();
    assert_eq!(s.remote_branches.len(), 1);
    assert_eq!(s.remote_branches[0].name, "origin/main");
    assert_eq!(s.upstream.as_deref(), Some("origin/main"));
    assert!(s.last_fetch.is_some());
    assert_eq!(
        git::load(
            f.dir.path(),
            &Config {
                show_remote_head: true,
                ..Config::default()
            }
        )
        .unwrap()
        .remote_branches
        .len(),
        2
    );
}

#[test]
fn merge_conflicts_and_rebase_state_are_visible() {
    let f = Fixture::new();
    let base = f.initial();
    f.repo
        .branch("topic", &f.repo.find_commit(base).unwrap(), false)
        .unwrap();
    f.write("file.txt", "main version\n");
    f.stage("file.txt");
    f.commit("Main changes");
    f.repo.set_head("refs/heads/topic").unwrap();
    f.repo
        .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    f.write("file.txt", "topic version\n");
    f.stage("file.txt");
    let topic = f.commit("Topic changes");
    f.repo.set_head("refs/heads/main").unwrap();
    f.repo
        .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    let annotated = f.repo.find_annotated_commit(topic).unwrap();
    f.repo.merge(&[&annotated], None, None).unwrap();
    let s = f.snapshot();
    assert_eq!(s.state, "MERGING");
    assert_eq!(s.staging.conflicted.len(), 1);
    assert!(s.staging.staged.is_empty());
    assert!(s.staging.unstaged.is_empty());
    f.repo.cleanup_state().unwrap();
    fs::create_dir(f.repo.path().join("rebase-merge")).unwrap();
    assert_eq!(git::state::load(&f.repo), "REBASING");
}

#[test]
fn linked_worktree_discovers_git_file_and_common_directory() {
    let f = Fixture::new();
    f.initial();
    let parent = tempfile::tempdir().unwrap();
    let worktree_path = parent.path().join("linked");
    f.repo.worktree("linked", &worktree_path, None).unwrap();
    assert!(worktree_path.join(".git").is_file());
    let s = git::load(&worktree_path, &Config::default()).unwrap();
    assert_ne!(s.git_dir, s.common_dir);
    assert_eq!(
        s.common_dir.canonicalize().unwrap(),
        f.repo.path().canonicalize().unwrap()
    );
    let target = WatchTarget {
        root: s.root,
        git_dir: s.git_dir,
        common_dir: s.common_dir.clone(),
    };
    assert!(watch::relevant(
        &target.common_dir.join("refs/heads/main"),
        &target,
        None
    ));
    assert!(!watch::relevant(
        &target.common_dir.join("objects/aa/object"),
        &target,
        None
    ));
}

fn metadata_tree(root: &Path) -> BTreeMap<PathBuf, (SystemTime, Vec<u8>)> {
    fn visit(root: &Path, path: &Path, out: &mut BTreeMap<PathBuf, (SystemTime, Vec<u8>)>) {
        for entry in fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            let metadata = path.metadata().unwrap();
            out.insert(
                path.strip_prefix(root).unwrap().to_path_buf(),
                (
                    metadata.modified().unwrap(),
                    if metadata.is_file() {
                        fs::read(&path).unwrap()
                    } else {
                        vec![]
                    },
                ),
            );
            if metadata.is_dir() {
                visit(root, &path, out);
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}

#[test]
fn full_snapshot_is_read_only_and_never_creates_index_lock() {
    use notify::Watcher;
    let f = Fixture::new();
    f.initial();
    // Refreshing a stale stat cache must still leave the on-disk index untouched.
    f.write("file.txt", "original\n");
    let before = metadata_tree(f.repo.path());
    let lock_seen = Arc::new(AtomicBool::new(false));
    let lock_events = lock_seen.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event
            && event
                .paths
                .iter()
                .any(|p| p.file_name().is_some_and(|n| n == "index.lock"))
        {
            lock_events.store(true, Ordering::SeqCst);
        }
    })
    .unwrap();
    // Sandboxed macOS processes may be unable to start an FSEvents stream.
    // Keep the byte/mtime comparison and lock sampler mandatory in that case.
    if let Err(error) = watcher.watch(f.repo.path(), notify::RecursiveMode::Recursive) {
        eprintln!("Native lock observation unavailable; using lock sampler: {error}");
    }
    let polling = Arc::new(AtomicBool::new(true));
    let poll_flag = polling.clone();
    let poll_seen = lock_seen.clone();
    let lock = f.repo.path().join("index.lock");
    let sampler = thread::spawn(move || {
        while poll_flag.load(Ordering::SeqCst) {
            if lock.exists() {
                poll_seen.store(true, Ordering::SeqCst);
            }
            thread::sleep(Duration::from_micros(100));
        }
    });
    for _ in 0..5 {
        let _ = f.snapshot();
        glasspane::summary::load(f.dir.path()).unwrap();
        glasspane::actions::amend::prepare(f.dir.path()).unwrap();
    }
    thread::sleep(Duration::from_millis(300));
    polling.store(false, Ordering::SeqCst);
    sampler.join().unwrap();
    assert!(!lock_seen.load(Ordering::SeqCst));
    assert_eq!(before, metadata_tree(f.repo.path()));
}

#[test]
fn graph_includes_decorations_and_detached_head() {
    let f = Fixture::new();
    let id = f.initial();
    f.repo.set_head_detached(id).unwrap();
    let graph = GitCliGraph.load(f.dir.path(), 10).unwrap();
    assert!(graph.iter().any(|line| {
        line.commit
            .as_ref()
            .is_some_and(|c| c.subject == "Initial commit" && c.is_head())
    }));
}

#[test]
fn graph_retains_other_branch_history_when_current_branch_is_unborn() {
    let f = Fixture::new();
    f.initial();
    f.repo.set_head("refs/heads/new-unborn").unwrap();
    let s = f.snapshot();
    assert!(s.commits.is_empty());
    assert!(s.graph.unwrap().iter().any(|line| {
        line.commit
            .as_ref()
            .is_some_and(|c| c.subject == "Initial commit")
    }));
}

#[test]
fn cherry_pick_and_bisect_states_are_visible() {
    let f = Fixture::new();
    let id = f.initial();
    fs::write(f.repo.path().join("CHERRY_PICK_HEAD"), format!("{id}\n")).unwrap();
    assert_eq!(git::state::load(&f.repo), "CHERRY-PICKING");
    fs::remove_file(f.repo.path().join("CHERRY_PICK_HEAD")).unwrap();
    fs::write(f.repo.path().join("BISECT_LOG"), "test bisect\n").unwrap();
    assert_eq!(git::state::load(&f.repo), "BISECTING");
}

#[test]
fn colored_merge_graph_keeps_routes_and_structured_labels() {
    use git::graph::LaneColor;
    let f = Fixture::new();
    let base_id = f.initial();
    f.write("file.txt", "main change\n");
    f.stage("file.txt");
    let main_id = f.commit("Main change");
    let base = f.repo.find_commit(base_id).unwrap();
    let main = f.repo.find_commit(main_id).unwrap();
    let signature = Signature::now("Test", "test@example.invalid").unwrap();
    let topic_id = f
        .repo
        .commit(
            Some("refs/heads/topic"),
            &signature,
            &signature,
            "Topic change",
            &base.tree().unwrap(),
            &[&base],
        )
        .unwrap();
    let topic = f.repo.find_commit(topic_id).unwrap();
    f.repo
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            "Merge topic",
            &main.tree().unwrap(),
            &[&main, &topic],
        )
        .unwrap();
    let rows = GitCliGraph.load(f.dir.path(), 20).unwrap();
    assert_eq!(rows.iter().filter(|r| r.commit.is_some()).count(), 4);
    assert!(rows.iter().flat_map(|r| &r.lanes).any(|c| c.symbol == '\\'));
    assert!(
        rows.iter()
            .flat_map(|r| &r.lanes)
            .any(|c| c.color == Some(LaneColor::Magenta))
    );
    assert!(
        rows.iter()
            .filter_map(|r| r.commit.as_ref())
            .any(|c| c.is_head() && c.subject == "Merge topic")
    );
    assert!(
        rows.iter()
            .filter_map(|r| r.commit.as_ref())
            .all(|c| !c.subject.contains('\x1b')
                && c.decorations.iter().all(|d| !d.contains('\x1b')))
    );
}
