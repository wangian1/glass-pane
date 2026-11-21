use git2::{Repository, Signature};
use glasspane::{
    actions::staging::{self, Operation, Request},
    git::{
        self,
        diff::{Kind, Target},
    },
};
use std::{fs, path::Path};

fn stage(repo: &Repository, path: &str) {
    let mut index = repo.index().unwrap();
    index.add_path(Path::new(path)).unwrap();
    index.write().unwrap();
}
fn commit(repo: &Repository) {
    let tree = repo
        .find_tree(repo.index().unwrap().write_tree().unwrap())
        .unwrap();
    let signature = Signature::now("Test", "test@example.invalid").unwrap();
    repo.commit(Some("HEAD"), &signature, &signature, "base", &tree, &[])
        .unwrap();
}
fn request(repo: &Repository, kind: Kind, operation: Operation) -> Request {
    let s = git::status::load(repo).unwrap();
    let file = match kind {
        Kind::Staged => &s.staged[0],
        Kind::Unstaged => &s.unstaged[0],
        Kind::Untracked => &s.untracked[0],
        Kind::Conflicted => &s.conflicted[0],
        Kind::Commit(_) => unreachable!(),
    };
    Request {
        root: repo.workdir().unwrap().into(),
        generation: 0,
        operation,
        expected_head: {
            let (branch, head) = git::head(repo).unwrap();
            (branch, head.map(|c| c.oid))
        },
        target: Target {
            kind,
            path: file.path.clone(),
            paths: file.paths.clone(),
        },
    }
}
#[test]
fn stages_one_literal_file_and_unstages_without_losing_edits() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    fs::write(dir.path().join("[x]"), "base\n").unwrap();
    stage(&repo, "[x]");
    commit(&repo);
    fs::write(dir.path().join("[x]"), "selected\n").unwrap();
    fs::write(dir.path().join("x"), "unrelated\n").unwrap();
    staging::execute(&request(&repo, Kind::Unstaged, Operation::Stage)).unwrap();
    assert_eq!(git::status::load(&repo).unwrap().staged.len(), 1);
    fs::write(dir.path().join("[x]"), "later unstaged edit\n").unwrap();
    staging::execute(&request(&repo, Kind::Staged, Operation::Unstage)).unwrap();
    assert!(git::status::load(&repo).unwrap().staged.is_empty());
    assert_eq!(
        fs::read_to_string(dir.path().join("[x]")).unwrap(),
        "later unstaged edit\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("x")).unwrap(),
        "unrelated\n"
    );
}
#[test]
fn unborn_unstage_preserves_partially_staged_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    fs::write(dir.path().join("-new → file"), "first\n").unwrap();
    staging::execute(&request(&repo, Kind::Untracked, Operation::Stage)).unwrap();
    fs::write(dir.path().join("-new → file"), "later\n").unwrap();
    staging::execute(&request(&repo, Kind::Staged, Operation::Unstage)).unwrap();
    let mut index = repo.index().unwrap();
    index.read(true).unwrap();
    assert!(index.is_empty());
    assert_eq!(
        fs::read_to_string(dir.path().join("-new → file")).unwrap(),
        "later\n"
    );
}
#[test]
fn renamed_and_deleted_files_stage_and_unstage_as_one_entry() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    fs::write(dir.path().join("old"), "rename contents\n").unwrap();
    stage(&repo, "old");
    commit(&repo);
    fs::rename(dir.path().join("old"), dir.path().join("new")).unwrap();
    staging::execute(&request(&repo, Kind::Unstaged, Operation::Stage)).unwrap();
    let s = git::status::load(&repo).unwrap();
    assert_eq!(s.staged[0].code, 'R');
    staging::execute(&request(&repo, Kind::Staged, Operation::Unstage)).unwrap();
    assert!(git::status::load(&repo).unwrap().staged.is_empty());
    assert!(dir.path().join("new").exists());
    assert!(!dir.path().join("old").exists());
    fs::remove_file(dir.path().join("new")).unwrap();
    staging::execute(&request(&repo, Kind::Unstaged, Operation::Stage)).unwrap();
    assert_eq!(git::status::load(&repo).unwrap().staged[0].code, 'D');
    staging::execute(&request(&repo, Kind::Staged, Operation::Unstage)).unwrap();
    assert!(!dir.path().join("old").exists());
}
#[test]
fn stale_selection_invalid_operations_and_index_lock_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    fs::write(dir.path().join("file"), "content\n").unwrap();
    let stale = request(&repo, Kind::Untracked, Operation::Stage);
    stage(&repo, "file");
    assert!(
        staging::execute(&stale)
            .unwrap_err()
            .to_string()
            .contains("status changed")
    );
    assert!(staging::execute(&request(&repo, Kind::Staged, Operation::Stage)).is_err());
    let branch = git::head(&repo).unwrap().0;
    let old_branch = request(&repo, Kind::Staged, Operation::Unstage);
    repo.set_head("refs/heads/changed-externally").unwrap();
    assert!(
        staging::execute(&old_branch)
            .unwrap_err()
            .to_string()
            .contains("HEAD or branch changed")
    );
    repo.set_head(&format!("refs/heads/{branch}")).unwrap();
    fs::write(repo.path().join("index.lock"), "external lock").unwrap();
    assert!(staging::execute(&request(&repo, Kind::Staged, Operation::Unstage)).is_err());
    assert_eq!(
        fs::read_to_string(repo.path().join("index.lock")).unwrap(),
        "external lock"
    );
    assert!(dir.path().join("file").exists());
}

#[test]
fn conflict_entries_cannot_be_accidentally_marked_resolved() {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path()).unwrap();
    fs::write(dir.path().join("file"), "conflict\n").unwrap();
    stage(&repo, "file");
    let mut index = repo.index().unwrap();
    let mut entry = index.get_path(Path::new("file"), 0).unwrap();
    index.remove_path(Path::new("file")).unwrap();
    entry.flags = (entry.flags & !0x3000) | 0x1000;
    index.add(&entry).unwrap();
    index.write().unwrap();
    let before = fs::read(repo.path().join("index")).unwrap();
    assert!(staging::execute(&request(&repo, Kind::Conflicted, Operation::Stage)).is_err());
    assert_eq!(before, fs::read(repo.path().join("index")).unwrap());
}
