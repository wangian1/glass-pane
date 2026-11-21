use git2::{Repository, Signature};
use glasspane::actions::amend::{self, Mode, Review};
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
};

struct Fixture {
    dir: tempfile::TempDir,
    root: PathBuf,
    repo: Repository,
}
impl Fixture {
    fn empty() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("repo with spaces");
        let repo = Repository::init(&root).unwrap();
        repo.set_head("refs/heads/main").unwrap();
        let hooks = dir.path().join("hooks");
        fs::create_dir(&hooks).unwrap();
        let mut config = repo.config().unwrap();
        config
            .set_str("core.hooksPath", hooks.to_str().unwrap())
            .unwrap();
        config.set_str("user.name", "Committer").unwrap();
        config
            .set_str("user.email", "commit@example.invalid")
            .unwrap();
        config.set_bool("commit.gpgSign", false).unwrap();
        Self { dir, root, repo }
    }
    fn new() -> Self {
        let f = Self::empty();
        f.commit("alpha\nbeta\n", "Original subject");
        f
    }
    fn stage(&self, text: &str) {
        fs::write(self.root.join("file.txt"), text).unwrap();
        let mut index = self.repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
    }
    fn commit(&self, text: &str, subject: &str) {
        self.stage(text);
        let mut index = self.repo.index().unwrap();
        let tree = self.repo.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = Signature::now("Original Author", "author@example.invalid").unwrap();
        let parent = self.repo.head().ok().and_then(|r| r.peel_to_commit().ok());
        self.repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                subject,
                &tree,
                &parent.iter().collect::<Vec<_>>(),
            )
            .unwrap();
    }
    fn head(&self) -> git2::Commit<'_> {
        self.repo.head().unwrap().peel_to_commit().unwrap()
    }
    fn editor(&self, body: &str) -> PathBuf {
        let path = self.dir.path().join("test editor.sh");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
    fn run(&self, mode: Mode, editor_body: &str) -> String {
        let plan = amend::prepare(&self.root).unwrap();
        let editor = self.editor(editor_body);
        let status = amend::command(&plan, mode)
            .unwrap()
            .env("GIT_EDITOR", format!("'{}'", editor.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        amend::finish(&plan, status).unwrap()
    }
}

#[test]
fn staged_amend_keeps_message_author_and_unstaged_work() {
    let f = Fixture::new();
    let old = f.head().id();
    f.stage("staged\nbeta\n");
    fs::write(f.root.join("file.txt"), "staged\nlocal edit\n").unwrap();
    let message = f.run(Mode::StagedKeepMessage, "exit 99");
    let head = f.head();
    assert_ne!(head.id(), old);
    assert_eq!(head.summary().unwrap(), Some("Original subject"));
    assert_eq!(head.author().name().unwrap(), "Original Author");
    assert_eq!(head.parent_count(), 0);
    let blob = head
        .tree()
        .unwrap()
        .get_path(Path::new("file.txt"))
        .unwrap()
        .id();
    assert_eq!(f.repo.find_blob(blob).unwrap().content(), b"staged\nbeta\n");
    assert_eq!(
        fs::read_to_string(f.root.join("file.txt")).unwrap(),
        "staged\nlocal edit\n"
    );
    let s = glasspane::git::status::load(&f.repo).unwrap();
    assert!(s.staged.is_empty());
    assert_eq!(s.unstaged.len(), 1);
    assert!(message.starts_with("Amended "));
}

#[test]
fn message_only_preserves_commit_tree_and_staged_contents() {
    let f = Fixture::new();
    let tree = f.head().tree_id();
    f.stage("staged change\n");
    fs::write(f.root.join("file.txt"), "local change\n").unwrap();
    let staged = f
        .repo
        .index()
        .unwrap()
        .get_path(Path::new("file.txt"), 0)
        .unwrap()
        .id;
    f.run(
        Mode::MessageOnly,
        "printf 'Reworded subject\\n\\nMessage body\\n' > \"$1\"",
    );
    let head = f.head();
    assert_eq!(head.tree_id(), tree);
    assert_eq!(head.summary().unwrap(), Some("Reworded subject"));
    assert!(head.message().unwrap().contains("Message body"));
    assert_eq!(head.author().name().unwrap(), "Original Author");
    assert_eq!(
        f.repo
            .index()
            .unwrap()
            .get_path(Path::new("file.txt"), 0)
            .unwrap()
            .id,
        staged
    );
    let s = glasspane::git::status::load(&f.repo).unwrap();
    assert_eq!(s.staged.len(), 1);
    assert_eq!(s.unstaged.len(), 1);
    assert_eq!(
        fs::read_to_string(f.root.join("file.txt")).unwrap(),
        "local change\n"
    );
}

#[test]
fn staged_and_message_amend_updates_both_and_keeps_parent() {
    let f = Fixture::new();
    let base = f.head().id();
    f.commit("second\n", "Second");
    f.stage("new staged content\n");
    f.run(Mode::StagedEditMessage, "printf 'New message\\n' > \"$1\"");
    assert_eq!(f.head().summary().unwrap(), Some("New message"));
    assert_eq!(f.head().parent_id(0).unwrap(), base);
    let blob = f
        .head()
        .tree()
        .unwrap()
        .get_path(Path::new("file.txt"))
        .unwrap()
        .id();
    assert_eq!(
        f.repo.find_blob(blob).unwrap().content(),
        b"new staged content\n"
    );
}

#[test]
fn rejects_no_head_and_active_operations() {
    let f = Fixture::empty();
    assert!(amend::prepare(&f.root).is_err());
    f.commit("base\n", "Initial");
    fs::create_dir(f.repo.path().join("rebase-merge")).unwrap();
    assert!(amend::prepare(&f.root).is_err());
}

#[test]
fn rejects_unmerged_index_even_without_operation_state() {
    let f = Fixture::new();
    let base = f.head().id();
    f.repo
        .branch("topic", &f.repo.find_commit(base).unwrap(), false)
        .unwrap();
    f.commit("main content\n", "Main");
    f.repo.set_head("refs/heads/topic").unwrap();
    f.repo
        .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    f.commit("topic content\n", "Topic");
    let topic = f.head().id();
    f.repo.set_head("refs/heads/main").unwrap();
    f.repo
        .checkout_head(Some(git2::build::CheckoutBuilder::new().force()))
        .unwrap();
    f.repo
        .merge(&[&f.repo.find_annotated_commit(topic).unwrap()], None, None)
        .unwrap();
    f.repo.cleanup_state().unwrap();
    assert!(amend::prepare(&f.root).is_err());
}

#[test]
fn rejects_changed_head_branch_or_index_after_review() {
    let f = Fixture::new();
    f.stage("staged\n");
    let plan = amend::prepare(&f.root).unwrap();
    f.stage("different staged content\n");
    assert!(amend::validate(&plan, Mode::MessageOnly).is_err());
    let plan = amend::prepare(&f.root).unwrap();
    f.repo.branch("same-commit", &f.head(), false).unwrap();
    f.repo.set_head("refs/heads/same-commit").unwrap();
    assert!(amend::validate(&plan, Mode::MessageOnly).is_err());
    let plan = amend::prepare(&f.root).unwrap();
    f.commit("next\n", "New HEAD");
    assert!(amend::validate(&plan, Mode::MessageOnly).is_err());
}

#[test]
fn cancelled_editor_and_failed_hook_leave_commit_unchanged() {
    let f = Fixture::new();
    f.stage("staged\n");
    let old = f.head().id();
    let editor = f.editor("exit 1");
    let plan = amend::prepare(&f.root).unwrap();
    let status = amend::command(&plan, Mode::MessageOnly)
        .unwrap()
        .env("GIT_EDITOR", format!("'{}'", editor.display()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(amend::finish(&plan, status).is_err());
    assert_eq!(f.head().id(), old);
    let hook = f.dir.path().join("hooks/pre-commit");
    fs::write(&hook, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(hook, fs::Permissions::from_mode(0o700)).unwrap();
    let status = amend::command(&plan, Mode::StagedKeepMessage)
        .unwrap()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(amend::finish(&plan, status).is_err());
    assert_eq!(f.head().id(), old);
    assert_eq!(
        glasspane::git::status::load(&f.repo).unwrap().staged.len(),
        1
    );
}

#[test]
fn no_staged_changes_selects_only_the_message_option() {
    let f = Fixture::new();
    let plan = amend::prepare(&f.root).unwrap();
    let mut review = Review::new(plan);
    assert_eq!(review.mode, Mode::MessageOnly);
    review.step(true);
    review.choose(Mode::StagedKeepMessage);
    assert_eq!(review.mode, Mode::MessageOnly);
    assert!(amend::command(&review.plan, Mode::StagedKeepMessage).is_err());
    assert!(amend::command(&review.plan, Mode::MessageOnly).is_ok());
}

#[test]
fn refreshing_only_index_stat_cache_does_not_invalidate_review() {
    let f = Fixture::new();
    let plan = amend::prepare(&f.root).unwrap();
    fs::write(f.root.join("file.txt"), "alpha\nbeta\n").unwrap();
    assert!(
        std::process::Command::new("git")
            .arg("-C")
            .arg(&f.root)
            .args(["update-index", "--refresh"])
            .status()
            .unwrap()
            .success()
    );
    assert!(amend::validate(&plan, Mode::MessageOnly).is_ok());
}
