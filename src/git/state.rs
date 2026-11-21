use git2::{Repository, RepositoryState};

pub fn load(repo: &Repository) -> &'static str {
    match repo.state() {
        RepositoryState::Merge => "MERGING",
        RepositoryState::Rebase
        | RepositoryState::RebaseInteractive
        | RepositoryState::RebaseMerge
        | RepositoryState::ApplyMailboxOrRebase => "REBASING",
        RepositoryState::CherryPick | RepositoryState::CherryPickSequence => "CHERRY-PICKING",
        RepositoryState::Revert | RepositoryState::RevertSequence => "REVERTING",
        RepositoryState::Bisect => "BISECTING",
        RepositoryState::ApplyMailbox => "APPLYING MAILBOX",
        _ if repo.path().join("BISECT_LOG").exists() => "BISECTING",
        _ => "",
    }
}
