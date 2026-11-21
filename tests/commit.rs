use git2::Repository;
use glasspane::actions::commit::{self, Editor};
use std::{
    fs,
    process::{Command, Stdio},
};
struct Fixture {
    dir: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        let f = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        f.git(&["init", "-b", "main"]);
        f.git(&["config", "user.name", "Test"]);
        f.git(&["config", "user.email", "test@example.invalid"]);
        f.git(&["config", "commit.gpgSign", "false"]);
        f.git(&[
            "config",
            "core.hooksPath",
            f.dir.path().join("hooks").to_str().unwrap(),
        ]);
        fs::write(f.dir.path().join("file"), "staged\n").unwrap();
        f.git(&["add", "file"]);
        f
    }
    fn git(&self, args: &[&str]) -> String {
        let result = Command::new("git")
            .arg("-C")
            .arg(self.dir.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8_lossy(&result.stdout).into()
    }
    fn hook(&self, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = self.dir.path().join("hooks/pre-commit");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
}
#[test]
fn first_commit_and_normal_commit_include_only_reviewed_index() {
    let f = Fixture::new();
    let plan = commit::prepare(f.dir.path()).unwrap();
    fs::write(f.dir.path().join("file"), "unstaged\n").unwrap();
    commit::execute(&plan, "First subject\n\nMessage body with Unicode 界").unwrap();
    assert_eq!(f.git(&["show", "HEAD:file"]), "staged\n");
    assert!(
        f.git(&["log", "-1", "--format=%B"])
            .contains("Message body with Unicode 界")
    );
    assert_eq!(
        fs::read_to_string(f.dir.path().join("file")).unwrap(),
        "unstaged\n"
    );
    f.git(&["add", "file"]);
    let plan = commit::prepare(f.dir.path()).unwrap();
    commit::execute(&plan, "Second commit").unwrap();
    assert_eq!(f.git(&["rev-list", "--count", "HEAD"]).trim(), "2");
}
#[test]
fn empty_message_and_changed_index_or_branch_are_refused() {
    let f = Fixture::new();
    let plan = commit::prepare(f.dir.path()).unwrap();
    assert!(commit::validate(&plan, " \n").is_err());
    fs::write(f.dir.path().join("file"), "new staged\n").unwrap();
    f.git(&["add", "file"]);
    assert!(commit::validate(&plan, "message").is_err());
    let plan = commit::prepare(f.dir.path()).unwrap();
    f.git(&["symbolic-ref", "HEAD", "refs/heads/other"]);
    assert!(commit::validate(&plan, "message").is_err());
    assert!(Repository::open(f.dir.path()).unwrap().head().is_err());
}
#[test]
fn hook_failure_preserves_head_and_message_can_be_retried() {
    let f = Fixture::new();
    let plan = commit::prepare(f.dir.path()).unwrap();
    f.hook("exit 1");
    assert!(commit::execute(&plan, "Keep this message").is_err());
    assert!(Repository::open(f.dir.path()).unwrap().head().is_err());
    f.hook("exit 0");
    commit::execute(&plan, "Keep this message").unwrap();
    assert_eq!(
        f.git(&["log", "-1", "--format=%s"]).trim(),
        "Keep this message"
    );
    assert!(commit::prepare(f.dir.path()).is_err());
}
#[test]
fn active_operation_and_conflicts_are_refused() {
    let f = Fixture::new();
    commit::execute(&commit::prepare(f.dir.path()).unwrap(), "base").unwrap();
    fs::write(f.dir.path().join("file"), "next\n").unwrap();
    f.git(&["add", "file"]);
    fs::write(
        f.dir.path().join(".git/MERGE_HEAD"),
        f.git(&["rev-parse", "HEAD"]),
    )
    .unwrap();
    assert!(commit::prepare(f.dir.path()).is_err());
    fs::remove_file(f.dir.path().join(".git/MERGE_HEAD")).unwrap();
    let repo = Repository::open(f.dir.path()).unwrap();
    let mut index = repo.index().unwrap();
    let mut entry = index.get_path(std::path::Path::new("file"), 0).unwrap();
    index.remove_all(["file"], None).unwrap();
    entry.flags = (entry.flags & !0x3000) | 0x1000;
    index.add(&entry).unwrap();
    index.write().unwrap();
    assert!(commit::prepare(f.dir.path()).is_err());
}
#[test]
fn editor_handles_multiline_unicode_and_cursor_edits() {
    let mut e = Editor::default();
    e.insert("first\n界e\u{301}🙂");
    e.left();
    e.backspace();
    assert_eq!(e.text, "first\n界🙂");
    e.home();
    e.delete();
    assert_eq!(e.text, "first\n🙂");
    e.vertical(false);
    e.insert("X");
    assert_eq!(e.text, "Xfirst\n🙂");
    e.end();
    e.insert("\u{1b}\0");
    assert!(!e.text.contains('\u{1b}'));
}
#[test]
fn native_commit_command_failure_does_not_claim_success() {
    let f = Fixture::new();
    let plan = commit::prepare(f.dir.path()).unwrap();
    let status = Command::new("false").stdin(Stdio::null()).status().unwrap();
    assert!(
        commit::finish(&plan, status)
            .unwrap_err()
            .to_string()
            .contains("unchanged")
    );
}

#[test]
fn commit_dialog_renders_review_editor_and_small_window_guard() {
    use glasspane::{app::App, config::Config, git, ui};
    use ratatui::{Terminal, backend::TestBackend};
    let f = Fixture::new();
    let mut app = App::new(f.dir.path().into());
    app.apply(0, git::load(f.dir.path(), &Config::default()));
    let mut review = commit::Review::new(commit::prepare(f.dir.path()).unwrap());
    review.editor.insert("Subject 界\n\nBody");
    app.commit = Some(review);
    for (width, height) in [(45, 16), (60, 20), (120, 36)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| ui::render(frame, &mut app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("Create commit"));
        if width < 60 {
            assert!(text.contains("Enlarge"));
        } else {
            assert!(text.contains("Ctrl+S"));
            assert!(text.contains("Subject 界"));
            assert!(text.contains("A file"));
        }
    }
    app.change_cwd(None);
    assert!(app.commit.is_none());
}
