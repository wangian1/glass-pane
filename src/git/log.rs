use git2::{Repository, Sort};

#[derive(Debug, Clone)]
pub struct CommitInfo {
    pub oid: git2::Oid,
    pub hash: String,
    pub author: String,
    pub timestamp: i64,
    pub subject: String,
}

pub fn info(commit: &git2::Commit<'_>) -> CommitInfo {
    CommitInfo {
        oid: commit.id(),
        hash: commit.id().to_string()[..7].to_string(),
        author: commit.author().name().unwrap_or("Unknown").to_string(),
        timestamp: commit.time().seconds(),
        subject: commit
            .summary()
            .ok()
            .flatten()
            .unwrap_or("(no subject)")
            .to_string(),
    }
}

pub fn load(repo: &Repository, limit: usize) -> Result<Vec<CommitInfo>, git2::Error> {
    match repo.head() {
        Err(e)
            if matches!(
                e.code(),
                git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
            ) =>
        {
            return Ok(vec![]);
        }
        Err(e) => return Err(e),
        Ok(_) => {}
    }
    let mut walk = repo.revwalk()?;
    walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
    walk.push_head()?;
    walk.take(limit)
        .map(|oid| Ok(info(&repo.find_commit(oid?)?)))
        .collect()
}
