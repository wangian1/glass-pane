use git2::{Repository, Signature};
use glasspane::{
    app::App,
    config::Config,
    event::Event,
    git::{
        self,
        diff::{self, Kind, Preview, Request, Target, View},
    },
};
use ratatui::{Terminal, backend::TestBackend};
use std::{fs, path::Path, sync::mpsc, time::Duration};

fn stage(repo: &Repository, name: &str) {
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(name)).unwrap();
    index.write().unwrap();
}
fn commit(repo: &Repository) {
    let oid = repo.index().unwrap().write_tree().unwrap();
    let tree = repo.find_tree(oid).unwrap();
    let sig = Signature::now("Test", "test@example.com").unwrap();
    repo.commit(Some("HEAD"), &sig, &sig, "initial", &tree, &[])
        .unwrap();
}
fn target(kind: Kind, path: &str) -> Target {
    Target {
        kind,
        path: path.into(),
        paths: vec![path.into()],
    }
}
fn text(root: &Path, target: &Target) -> String {
    diff::load(root, target).unwrap().lines.join("\n")
}

#[test]
fn partial_staging_is_separated_and_diff_reads_do_not_write_index_or_head() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    let path = dir.path().join("file.txt");
    fs::write(&path, "original\n").unwrap();
    stage(&repo, "file.txt");
    commit(&repo);
    fs::write(&path, "staged\n").unwrap();
    stage(&repo, "file.txt");
    fs::write(&path, "unstaged\n").unwrap();
    let index = fs::read(repo.path().join("index")).unwrap();
    let modified = fs::metadata(repo.path().join("index"))
        .unwrap()
        .modified()
        .unwrap();
    let head = repo.head().unwrap().target();
    let staged = text(dir.path(), &target(Kind::Staged, "file.txt"));
    assert!(staged.contains("-original") && staged.contains("+staged"));
    assert!(!staged.contains("+unstaged"));
    let unstaged = text(dir.path(), &target(Kind::Unstaged, "file.txt"));
    assert!(unstaged.contains("-staged") && unstaged.contains("+unstaged"));
    assert_eq!(index, fs::read(repo.path().join("index")).unwrap());
    assert_eq!(
        modified,
        fs::metadata(repo.path().join("index"))
            .unwrap()
            .modified()
            .unwrap()
    );
    assert_eq!(head, repo.head().unwrap().target());
    assert!(!repo.path().join("index.lock").exists());
}

#[test]
fn unborn_untracked_binary_deleted_and_literal_paths() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    fs::write(dir.path().join("[x].txt"), "selected\n").unwrap();
    fs::write(dir.path().join("x.txt"), "unrelated\n").unwrap();
    let patch = text(dir.path(), &target(Kind::Untracked, "[x].txt"));
    assert!(patch.contains("+selected"));
    assert!(!patch.contains("unrelated"));
    stage(&repo, "[x].txt");
    assert!(text(dir.path(), &target(Kind::Staged, "[x].txt")).contains("+selected"));
    commit(&repo);
    fs::remove_file(dir.path().join("[x].txt")).unwrap();
    assert!(text(dir.path(), &target(Kind::Unstaged, "[x].txt")).contains("-selected"));
    fs::write(dir.path().join("binary"), [0, 1, 2, 0]).unwrap();
    assert!(text(dir.path(), &target(Kind::Untracked, "binary")).contains("Binary"));
}

#[test]
fn rename_uses_real_paths_and_preview_is_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    fs::write(dir.path().join("old"), "renamed content\n").unwrap();
    stage(&repo, "old");
    commit(&repo);
    fs::rename(dir.path().join("old"), dir.path().join("new → name")).unwrap();
    let mut index = repo.index().unwrap();
    index.remove_path(Path::new("old")).unwrap();
    index.write().unwrap();
    stage(&repo, "new → name");
    let s = git::status::load(&repo).unwrap();
    let file = &s.staged[0];
    assert_eq!(file.code, 'R');
    let patch = text(
        dir.path(),
        &Target {
            kind: Kind::Staged,
            path: file.path.clone(),
            paths: file.paths.clone(),
        },
    );
    assert!(patch.contains("renamed content"));
    fs::write(dir.path().join("many"), "line\n".repeat(6000)).unwrap();
    let preview = diff::load(dir.path(), &target(Kind::Untracked, "many")).unwrap();
    assert!(preview.lines.len() <= 4001);
    assert!(preview.lines.last().unwrap().contains("Preview limited"));
}

#[test]
fn staging_navigation_full_screen_and_directory_changes() {
    let dir = tempfile::tempdir().unwrap();
    Repository::init(dir.path()).unwrap();
    for i in 0..40 {
        fs::write(dir.path().join(format!("file{i:02}")), "new\n").unwrap();
    }
    let mut app = App::new(dir.path().into());
    app.apply(0, git::load(dir.path(), &Config::default()));
    app.focus = 3;
    let first = app.selected_file.clone();
    app.scroll_by(true);
    assert_ne!(first, app.selected_file);
    app.go_edge(true);
    let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
    terminal
        .draw(|f| glasspane::ui::render(f, &mut app))
        .unwrap();
    assert!(app.scroll[3] > 0);
    let selected = app.selected_file.clone().unwrap();
    app.diff = Some(View {
        id: 1,
        target: selected.clone(),
        result: Some(Ok(Preview {
            lines: vec!["+hello\u{1b}[31m".into(); 80],
        })),
        scroll: 79,
        horizontal: 0,
        max_scroll: 0,
    });
    app.no_color = true;
    for (width, height) in [(20, 5), (80, 24), (140, 40)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| glasspane::ui::render(f, &mut app))
            .unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(!content.contains('\u{1b}'));
        assert!(!content.contains("Branch Graph"));
    }
    app.diff = None;
    assert_eq!(app.selected_file, Some(selected));
    app.change_cwd(None);
    assert!(app.diff.is_none());
}

#[test]
fn background_worker_returns_tagged_results() {
    let dir = tempfile::tempdir().unwrap();
    Repository::init(dir.path()).unwrap();
    fs::write(dir.path().join("file"), "background\n").unwrap();
    let (tx, rx) = mpsc::channel();
    let worker = diff::Worker::spawn(tx);
    worker.request(Request {
        root: dir.path().into(),
        target: target(Kind::Untracked, "file"),
        id: 42,
    });
    match rx.recv_timeout(Duration::from_secs(5)).unwrap() {
        Event::DiffLoaded { id, result } => {
            assert_eq!(id, 42);
            assert!(result.unwrap().lines.join("\n").contains("+background"));
        }
        _ => panic!("unexpected event"),
    }
}

#[test]
fn stale_results_cannot_replace_a_reopened_view_or_cross_directories() {
    let dir = tempfile::tempdir().unwrap();
    Repository::init(dir.path()).unwrap();
    fs::write(dir.path().join("file"), "one\n").unwrap();
    let mut app = App::new(dir.path().into());
    app.apply(0, git::load(dir.path(), &Config::default()));
    let (tx, _rx) = mpsc::channel();
    let worker = diff::Worker::spawn(tx);
    app.open_diff(&worker);
    let old = app.diff.as_ref().unwrap().id;
    app.diff = None;
    app.open_diff(&worker);
    let new = app.diff.as_ref().unwrap().id;
    assert_ne!(old, new);
    app.apply_diff(old, Err("stale".into()));
    assert!(app.diff.as_ref().unwrap().result.is_none());
    app.apply_diff(
        new,
        Ok(Preview {
            lines: vec!["current".into()],
        }),
    );
    assert!(app.diff.as_ref().unwrap().result.as_ref().unwrap().is_ok());
    app.change_cwd(Some(dir.path().join("different")));
    app.apply_diff(new, Err("old directory".into()));
    assert!(app.diff.is_none());
}
