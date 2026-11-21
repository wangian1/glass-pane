//! Immutable commit inspection. Merge commits are compared with their first parent.
use super::diff::{Kind, Target};
use git2::{Oid, Repository};
use std::path::Path;

#[derive(Debug)]
pub struct Inspection {
    pub oid: Oid,
    pub author: String,
    pub timestamp: i64,
    pub parents: Vec<Oid>,
    pub message: String,
    pub files: Vec<Target>,
    pub truncated: bool,
}
#[derive(Debug)]
pub struct View {
    pub id: u64,
    pub oid: Oid,
    pub result: Option<Result<Inspection, String>>,
    pub selected: usize,
    pub scroll: usize,
    pub message_scroll: usize,
    pub max_message_scroll: usize,
    pub message_focus: bool,
}
impl View {
    pub fn new(id: u64, oid: Oid) -> Self {
        Self {
            id,
            oid,
            result: None,
            selected: 0,
            scroll: 0,
            message_scroll: 0,
            max_message_scroll: 0,
            message_focus: false,
        }
    }
    pub fn file(&self) -> Option<&Target> {
        self.result
            .as_ref()?
            .as_ref()
            .ok()?
            .files
            .get(self.selected)
    }
    pub fn navigate(&mut self, down: bool, step: usize) {
        if self.message_focus {
            self.message_scroll = if down {
                self.message_scroll
                    .saturating_add(step)
                    .min(self.max_message_scroll)
            } else {
                self.message_scroll.saturating_sub(step)
            };
        } else {
            let count = self
                .result
                .as_ref()
                .and_then(|r| r.as_ref().ok())
                .map(|r| r.files.len())
                .unwrap_or(0);
            self.selected = if down {
                self.selected
                    .saturating_add(step)
                    .min(count.saturating_sub(1))
            } else {
                self.selected.saturating_sub(step)
            };
        }
    }
}
pub fn load(root: &Path, oid: Oid) -> Result<Inspection, git2::Error> {
    let repo = Repository::open(root)?;
    let commit = repo.find_commit(oid)?;
    let tree = commit.tree()?;
    let parent = if commit.parent_count() > 0 {
        Some(commit.parent(0)?.tree()?)
    } else {
        None
    };
    let mut diff = repo.diff_tree_to_tree(parent.as_ref(), Some(&tree), None)?;
    diff.find_similar(None)?;
    let count = diff.deltas().len();
    let files = diff
        .deltas()
        .take(5000)
        .map(|d| {
            let old = d.old_file().path().map(Path::to_path_buf);
            let new = d.new_file().path().map(Path::to_path_buf);
            let path = if d.status() == git2::Delta::Renamed {
                format!(
                    "R {} → {}",
                    old.as_ref()
                        .map(|p| p.to_string_lossy())
                        .unwrap_or_default(),
                    new.as_ref()
                        .map(|p| p.to_string_lossy())
                        .unwrap_or_default()
                )
            } else {
                format!(
                    "{} {}",
                    match d.status() {
                        git2::Delta::Added => "A",
                        git2::Delta::Deleted => "D",
                        git2::Delta::Typechange => "T",
                        _ => "M",
                    },
                    new.as_ref()
                        .or(old.as_ref())
                        .map(|p| p.to_string_lossy())
                        .unwrap_or_default()
                )
            };
            Target {
                kind: Kind::Commit(oid),
                path,
                paths: [old, new].into_iter().flatten().collect(),
            }
        })
        .collect();
    let message = String::from_utf8_lossy(commit.message_bytes());
    let mut message: String = message.chars().take(64 * 1024).collect();
    if commit.message_bytes().len() > message.len() {
        message.push_str("\n… Commit message preview limited.");
    }
    Ok(Inspection {
        oid,
        author: format!(
            "{} <{}>",
            commit.author().name().unwrap_or("Unknown"),
            commit.author().email().unwrap_or("")
        ),
        timestamp: commit.time().seconds(),
        parents: commit.parent_ids().collect(),
        message,
        files,
        truncated: count > 5000,
    })
}
