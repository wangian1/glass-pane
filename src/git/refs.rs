use crate::config::BranchSort;
use git2::{BranchType, Repository};

#[derive(Debug, Clone)]
pub struct BranchInfo {
    pub name: String,
    pub hash: String,
    pub timestamp: i64,
    pub current: bool,
}

pub fn load(
    repo: &Repository,
    sort: BranchSort,
    show_remote_head: bool,
) -> Result<(Vec<BranchInfo>, Vec<BranchInfo>), git2::Error> {
    let mut local = vec![];
    let mut remote = vec![];
    for entry in repo.branches(None)? {
        let (branch, kind) = entry?;
        let reference = branch.get();
        if kind == BranchType::Remote
            && reference.symbolic_target_bytes().is_some()
            && !show_remote_head
        {
            continue;
        }
        let name = branch.name()?.unwrap_or("(non-UTF8 branch)").to_string();
        let commit = reference.peel_to_commit()?;
        let row = BranchInfo {
            name,
            hash: commit.id().to_string()[..7].to_string(),
            timestamp: commit.time().seconds(),
            current: branch.is_head(),
        };
        if kind == BranchType::Local {
            local.push(row);
        } else {
            remote.push(row);
        }
    }
    for rows in [&mut local, &mut remote] {
        rows.sort_by(|a, b| {
            b.current.cmp(&a.current).then_with(|| match sort {
                BranchSort::Date => b.timestamp.cmp(&a.timestamp).then(a.name.cmp(&b.name)),
                BranchSort::Name => a.name.cmp(&b.name),
            })
        });
    }
    Ok((local, remote))
}
