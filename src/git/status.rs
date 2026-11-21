use git2::{Delta, Repository, Status, StatusOptions};

#[derive(Debug, Clone)]
pub struct FileStatus {
    pub paths: Vec<std::path::PathBuf>,
    pub code: char,
    pub path: String,
}

#[derive(Debug, Clone, Default)]
pub struct Staging {
    pub conflicted: Vec<FileStatus>,
    pub staged: Vec<FileStatus>,
    pub unstaged: Vec<FileStatus>,
    pub untracked: Vec<FileStatus>,
}

fn row(delta: Option<git2::DiffDelta<'_>>, fallback: &str, code: char) -> FileStatus {
    let paths = delta
        .as_ref()
        .map(|d| {
            [d.old_file().path(), d.new_file().path()]
                .into_iter()
                .flatten()
                .map(std::path::Path::to_path_buf)
                .collect()
        })
        .unwrap_or_else(|| vec![std::path::PathBuf::from(fallback)]);
    let path = delta
        .map(|d| {
            let old = d
                .old_file()
                .path()
                .map(|p| p.to_string_lossy().into_owned());
            let new = d
                .new_file()
                .path()
                .map(|p| p.to_string_lossy().into_owned());
            if d.status() == Delta::Renamed {
                format!(
                    "{} → {}",
                    old.as_deref().unwrap_or(fallback),
                    new.as_deref().unwrap_or(fallback)
                )
            } else {
                new.or(old).unwrap_or_else(|| fallback.to_string())
            }
        })
        .unwrap_or_else(|| fallback.to_string());
    FileStatus { code, path, paths }
}

pub fn load(repo: &Repository) -> Result<Staging, git2::Error> {
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true)
        .update_index(false);
    let statuses = repo.statuses(Some(&mut options))?;
    let mut result = Staging::default();
    for entry in statuses.iter() {
        let s = entry.status();
        let path = String::from_utf8_lossy(entry.path_bytes());
        if s.contains(Status::CONFLICTED) {
            result.conflicted.push(row(None, &path, 'U'));
            continue;
        }
        let staged = if s.contains(Status::INDEX_RENAMED) {
            Some('R')
        } else if s.contains(Status::INDEX_NEW) {
            Some('A')
        } else if s.contains(Status::INDEX_DELETED) {
            Some('D')
        } else if s.contains(Status::INDEX_TYPECHANGE) {
            Some('T')
        } else if s.contains(Status::INDEX_MODIFIED) {
            Some('M')
        } else {
            None
        };
        if let Some(code) = staged {
            result.staged.push(row(entry.head_to_index(), &path, code));
        }
        let unstaged = if s.contains(Status::WT_RENAMED) {
            Some('R')
        } else if s.contains(Status::WT_DELETED) {
            Some('D')
        } else if s.contains(Status::WT_TYPECHANGE) {
            Some('T')
        } else if s.intersects(Status::WT_MODIFIED | Status::WT_UNREADABLE) {
            Some('M')
        } else {
            None
        };
        if let Some(code) = unstaged {
            result
                .unstaged
                .push(row(entry.index_to_workdir(), &path, code));
        }
        if s.contains(Status::WT_NEW) {
            result
                .untracked
                .push(row(entry.index_to_workdir(), &path, '?'));
        }
    }
    Ok(result)
}
