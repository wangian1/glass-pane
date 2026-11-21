use git2::{Repository, Signature};
use glasspane::actions::{BranchTarget, switch_branch};
use std::{fs, path::Path};

struct Fixture {
    dir: tempfile::TempDir,
    repo: Repository,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        repo.set_head("refs/heads/main").unwrap();
        let hooks = dir.path().join("empty-hooks");
        fs::create_dir(&hooks).unwrap();
        repo.config()
            .unwrap()
            .set_str("core.hooksPath", hooks.to_str().unwrap())
            .unwrap();
        let f = Self { dir, repo };
        fs::write(f.dir.path().join("shared.txt"), "common\n").unwrap();
        let mut index = f.repo.index().unwrap();
        index.add_path(Path::new("shared.txt")).unwrap();
        index.write().unwrap();
        f.commit("base\n", "Base");
        f.repo
            .branch(
                "topic",
                &f.repo.head().unwrap().peel_to_commit().unwrap(),
                false,
            )
            .unwrap();
        f.commit("main version\n", "Main change");
        f
    }
    fn commit(&self, text: &str, subject: &str) {
        fs::write(self.dir.path().join("file.txt"), text).unwrap();
        let mut index = self.repo.index().unwrap();
        index.add_path(Path::new("file.txt")).unwrap();
        index.write().unwrap();
        let tree = self.repo.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = Signature::now("Test", "test@example.invalid").unwrap();
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
    fn target(name: &str, remote: bool) -> BranchTarget {
        BranchTarget {
            name: name.into(),
            remote,
        }
    }
    fn head(&self) -> String {
        self.repo.head().unwrap().shorthand().unwrap().into()
    }
}

#[test]
fn local_switch_updates_head_and_worktree() {
    let f = Fixture::new();
    let message = switch_branch(f.dir.path(), &Fixture::target("topic", false)).unwrap();
    assert_eq!(message, "Switched to topic");
    assert_eq!(f.head(), "topic");
    assert_eq!(
        fs::read_to_string(f.dir.path().join("file.txt")).unwrap(),
        "base\n"
    );
}

#[test]
fn refuses_to_overwrite_unstaged_or_staged_changes() {
    for staged in [false, true] {
        let f = Fixture::new();
        fs::write(f.dir.path().join("file.txt"), "user changes\n").unwrap();
        if staged {
            let mut index = f.repo.index().unwrap();
            index.add_path(Path::new("file.txt")).unwrap();
            index.write().unwrap();
        }
        let index_before = fs::read(f.repo.path().join("index")).unwrap();
        assert!(switch_branch(f.dir.path(), &Fixture::target("topic", false)).is_err());
        assert_eq!(f.head(), "main");
        assert_eq!(
            fs::read_to_string(f.dir.path().join("file.txt")).unwrap(),
            "user changes\n"
        );
        assert_eq!(fs::read(f.repo.path().join("index")).unwrap(), index_before);
    }
}

#[test]
fn preserves_untracked_files_and_compatible_local_changes() {
    let f = Fixture::new();
    fs::write(f.dir.path().join("notes.txt"), "keep me").unwrap();
    fs::write(f.dir.path().join("shared.txt"), "keep tracked changes").unwrap();
    switch_branch(f.dir.path(), &Fixture::target("topic", false)).unwrap();
    assert_eq!(
        fs::read_to_string(f.dir.path().join("notes.txt")).unwrap(),
        "keep me"
    );
    assert_eq!(
        fs::read_to_string(f.dir.path().join("shared.txt")).unwrap(),
        "keep tracked changes"
    );
}

#[test]
fn refuses_branch_checked_out_in_another_worktree() {
    let f = Fixture::new();
    let parent = tempfile::tempdir().unwrap();
    let branch = f.repo.find_reference("refs/heads/topic").unwrap();
    let mut options = git2::WorktreeAddOptions::new();
    options.reference(Some(&branch));
    f.repo
        .worktree("linked", &parent.path().join("linked"), Some(&options))
        .unwrap();
    assert!(switch_branch(f.dir.path(), &Fixture::target("topic", false)).is_err());
    assert_eq!(f.head(), "main");
}

#[test]
fn remote_selection_creates_tracking_branch_and_reuses_it() {
    let f = Fixture::new();
    f.repo.remote("origin", "/unused/no-network").unwrap();
    let tip = f
        .repo
        .find_branch("topic", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    f.repo
        .reference("refs/remotes/origin/feature/topic", tip, true, "test")
        .unwrap();
    let target = Fixture::target("origin/feature/topic", true);
    let message = switch_branch(f.dir.path(), &target).unwrap();
    assert!(message.contains("tracking origin/feature/topic"));
    assert_eq!(f.head(), "feature/topic");
    assert_eq!(
        f.repo
            .find_branch("feature/topic", git2::BranchType::Local)
            .unwrap()
            .upstream()
            .unwrap()
            .name()
            .unwrap(),
        Some("origin/feature/topic")
    );
    switch_branch(f.dir.path(), &Fixture::target("main", false)).unwrap();
    switch_branch(f.dir.path(), &target).unwrap();
    assert_eq!(f.head(), "feature/topic");
    assert!(!f.repo.path().join("FETCH_HEAD").exists());
}

#[test]
fn remote_name_collision_and_failed_tracking_checkout_leave_refs_intact() {
    let f = Fixture::new();
    f.repo.remote("origin", "/unused/no-network").unwrap();
    let tip = f
        .repo
        .find_branch("topic", git2::BranchType::Local)
        .unwrap()
        .get()
        .target()
        .unwrap();
    f.repo
        .reference("refs/remotes/origin/topic", tip, true, "test")
        .unwrap();
    assert!(
        switch_branch(f.dir.path(), &Fixture::target("origin/topic", true))
            .unwrap_err()
            .to_string()
            .contains("already exists")
    );
    f.repo
        .reference("refs/remotes/origin/new-topic", tip, true, "test")
        .unwrap();
    fs::write(f.dir.path().join("file.txt"), "user changes\n").unwrap();
    assert!(switch_branch(f.dir.path(), &Fixture::target("origin/new-topic", true)).is_err());
    assert!(
        f.repo
            .find_branch("new-topic", git2::BranchType::Local)
            .is_err()
    );
    assert_eq!(f.head(), "main");
}

#[test]
fn symbolic_remote_head_and_deleted_branches_are_not_checked_out() {
    let f = Fixture::new();
    f.repo
        .reference_symbolic("refs/remotes/origin/HEAD", "refs/heads/main", true, "test")
        .unwrap();
    assert!(switch_branch(f.dir.path(), &Fixture::target("origin/HEAD", true)).is_err());
    assert!(switch_branch(f.dir.path(), &Fixture::target("deleted", false)).is_err());
    assert_eq!(f.head(), "main");
}

#[test]
fn failed_post_checkout_hook_reports_the_actual_switched_branch() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    let hook = f.dir.path().join("empty-hooks/post-checkout");
    fs::write(&hook, "#!/bin/sh\necho 'test hook failed' >&2\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let message = switch_branch(f.dir.path(), &Fixture::target("topic", false)).unwrap();
    assert_eq!(f.head(), "topic");
    assert!(message.contains("Switched to topic"));
    assert!(message.contains("test hook failed"));
}

#[test]
fn refuses_to_switch_during_a_rebase() {
    let f = Fixture::new();
    fs::create_dir(f.repo.path().join("rebase-merge")).unwrap();
    assert!(switch_branch(f.dir.path(), &Fixture::target("topic", false)).is_err());
    assert_eq!(f.head(), "main");
    assert_eq!(
        fs::read_to_string(f.dir.path().join("file.txt")).unwrap(),
        "main version\n"
    );
}
