use git2::{Oid, Repository, Signature};
use glasspane::{
    app::App,
    config::Config,
    git::{self, diff, inspect},
};
use ratatui::{Terminal, backend::TestBackend};
use std::{fs, path::Path, sync::mpsc, time::Duration};
fn commit(repo: &Repository, message: &str, parents: &[Oid]) -> Oid {
    let tree = repo
        .find_tree(repo.index().unwrap().write_tree().unwrap())
        .unwrap();
    let sig = Signature::now("Test", "test@example.invalid").unwrap();
    let parents: Vec<_> = parents
        .iter()
        .map(|id| repo.find_commit(*id).unwrap())
        .collect();
    repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        message,
        &tree,
        &parents.iter().collect::<Vec<_>>(),
    )
    .unwrap()
}
fn add(repo: &Repository, name: &str, value: &str) {
    fs::write(repo.workdir().unwrap().join(name), value).unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(name)).unwrap();
    index.write().unwrap();
}
#[test]
fn root_and_normal_commit_diffs_use_commit_trees_without_index_or_worktree_content() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    add(&repo, "file", "original\n");
    let root = commit(&repo, "Root subject\n\nFull body", &[]);
    add(&repo, "file", "committed\n");
    let second = commit(&repo, "Second", &[root]);
    add(&repo, "file", "unrelated staged\n");
    fs::write(dir.path().join("file"), "unrelated worktree\n").unwrap();
    let index_before = fs::read(repo.path().join("index")).unwrap();
    let time_before = fs::metadata(repo.path().join("index"))
        .unwrap()
        .modified()
        .unwrap();
    let root_info = inspect::load(dir.path(), root).unwrap();
    assert!(root_info.parents.is_empty());
    assert!(root_info.message.contains("Full body"));
    assert!(
        diff::load(dir.path(), &root_info.files[0])
            .unwrap()
            .lines
            .join("\n")
            .contains("+original")
    );
    let info = inspect::load(dir.path(), second).unwrap();
    let patch = diff::load(dir.path(), &info.files[0])
        .unwrap()
        .lines
        .join("\n");
    assert!(patch.contains("-original") && patch.contains("+committed"));
    assert!(!patch.contains("unrelated"));
    assert_eq!(index_before, fs::read(repo.path().join("index")).unwrap());
    assert_eq!(
        time_before,
        fs::metadata(repo.path().join("index"))
            .unwrap()
            .modified()
            .unwrap()
    );
    assert_eq!(repo.head().unwrap().target(), Some(second));
    assert!(!repo.path().join("index.lock").exists());
}
#[test]
fn merge_comparison_uses_first_parent_and_empty_commits_have_no_files() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    add(&repo, "file", "base\n");
    let root = commit(&repo, "root", &[]);
    add(&repo, "file", "main\n");
    let first = commit(&repo, "first", &[root]);
    let empty = commit(&repo, "empty", &[first]);
    assert!(inspect::load(dir.path(), empty).unwrap().files.is_empty());
    let merge = commit(&repo, "merge", &[empty, root]);
    let info = inspect::load(dir.path(), merge).unwrap();
    assert_eq!(info.parents, vec![empty, root]);
    assert!(info.files.is_empty());
}
#[test]
fn rename_paths_and_deleted_binary_files_are_inspectable() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    add(&repo, "old", "contents\n");
    add(&repo, "deleted", "removed\n");
    let root = commit(&repo, "root", &[]);
    fs::rename(dir.path().join("old"), dir.path().join("new → file")).unwrap();
    fs::remove_file(dir.path().join("deleted")).unwrap();
    let mut index = repo.index().unwrap();
    index.remove_path(Path::new("old")).unwrap();
    index.remove_path(Path::new("deleted")).unwrap();
    index.add_path(Path::new("new → file")).unwrap();
    index.write().unwrap();
    add(&repo, "binary", "\0binary\n");
    let id = commit(&repo, "changes", &[root]);
    let info = inspect::load(dir.path(), id).unwrap();
    assert_eq!(info.files.len(), 3);
    let rename = info
        .files
        .iter()
        .find(|f| f.path.starts_with("R "))
        .unwrap();
    assert!(rename.paths.contains(&"old".into()) && rename.paths.contains(&"new → file".into()));
    for file in &info.files {
        assert!(!diff::load(dir.path(), file).unwrap().lines.is_empty());
    }
}
#[test]
fn inspector_selection_return_small_layouts_and_stale_results() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    add(&repo, "file", "first\n");
    let first = commit(&repo, "first\n\nBody", &[]);
    add(&repo, "file", "second\n");
    let second = commit(&repo, "second", &[first]);
    let mut app = App::new(dir.path().into());
    app.apply(0, git::load(dir.path(), &Config::default()));
    app.focus = 2;
    assert_eq!(app.selected_commit, Some(second));
    app.scroll_by(true);
    assert_eq!(app.selected_commit, Some(first));
    let (tx, rx) = mpsc::channel();
    let worker = diff::Worker::spawn(tx);
    app.commit_preparing = true;
    app.open_inspection(&worker);
    assert!(
        app.inspection.is_none(),
        "a preparing commit review must not be hidden by inspection"
    );
    app.commit_preparing = false;
    app.open_inspection(&worker);
    let id = app.inspection.as_ref().unwrap().id;
    app.apply_inspection(id + 1, Err("stale".into()));
    assert!(app.inspection.as_ref().unwrap().result.is_none());
    match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
        glasspane::event::Event::InspectionLoaded { id, result } => {
            app.apply_inspection(id, result)
        }
        _ => panic!("unexpected event"),
    }
    for (width, height) in [(20, 5), (80, 24), (140, 40)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| glasspane::ui::render(f, &mut app))
            .unwrap();
    }
    app.open_commit_diff(&worker);
    assert!(app.diff.is_some());
    app.diff = None;
    assert!(app.inspection.is_some());
    app.inspection = None;
    assert_eq!(app.selected_commit, Some(first));
    app.open_inspection(&worker);
    app.change_cwd(None);
    app.apply_inspection(id, Err("old directory".into()));
    assert!(app.inspection.is_none());
}
