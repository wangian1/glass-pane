use crate::event::Event;
use git2::{DiffFormat, DiffOptions, Repository};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Sender},
    thread,
};

const MAX_BYTES: usize = 256 * 1024;
const MAX_LINES: usize = 4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Commit(git2::Oid),
    Conflicted,
    Staged,
    Unstaged,
    Untracked,
}
impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Commit(_) => "COMMIT · first parent → commit (root: empty tree)",
            Self::Staged => "STAGED · HEAD → index",
            Self::Unstaged => "UNSTAGED · index → working tree",
            Self::Untracked => "UNTRACKED · new file",
            Self::Conflicted => "CONFLICTED · unresolved file",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub kind: Kind,
    pub path: String,
    pub paths: Vec<PathBuf>,
}
#[derive(Debug)]
pub struct Preview {
    pub lines: Vec<String>,
}
#[derive(Debug)]
pub struct View {
    pub id: u64,
    pub target: Target,
    pub result: Option<Result<Preview, String>>,
    pub scroll: usize,
    pub horizontal: u16,
    pub max_scroll: usize,
}
#[derive(Clone)]
pub struct Request {
    pub root: PathBuf,
    pub target: Target,
    pub id: u64,
}

enum Job {
    Diff(Request),
    Inspect {
        id: u64,
        root: PathBuf,
        oid: git2::Oid,
    },
}
pub struct Worker {
    tx: Option<Sender<Job>>,
    handle: Option<thread::JoinHandle<()>>,
}
impl Worker {
    pub fn spawn(events: Sender<Event>) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let handle = thread::spawn(move || {
            while let Ok(mut request) = rx.recv() {
                for newer in rx.try_iter() {
                    request = newer;
                }
                let event = match request {
                    Job::Diff(request) => Event::DiffLoaded {
                        id: request.id,
                        result: load(&request.root, &request.target).map_err(|e| e.to_string()),
                    },
                    Job::Inspect { id, root, oid } => Event::InspectionLoaded {
                        id,
                        result: super::inspect::load(&root, oid).map_err(|e| e.to_string()),
                    },
                };
                if events.send(event).is_err() {
                    break;
                }
            }
        });
        Self {
            tx: Some(tx),
            handle: Some(handle),
        }
    }
    pub fn request(&self, request: Request) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Job::Diff(request));
        }
    }
    pub fn inspect(&self, id: u64, root: PathBuf, oid: git2::Oid) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Job::Inspect { id, root, oid });
        }
    }
    pub fn is_finished(&self) -> bool {
        self.handle
            .as_ref()
            .is_some_and(thread::JoinHandle::is_finished)
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn load(root: &std::path::Path, target: &Target) -> Result<Preview, git2::Error> {
    if target.kind == Kind::Conflicted {
        return Ok(Preview {
            lines: vec![
                "Unresolved conflict. Open the file in your editor to review conflict markers."
                    .into(),
            ],
        });
    }
    let repo = Repository::open(root)?;
    let mut options = DiffOptions::new();
    options
        .disable_pathspec_match(true)
        .context_lines(3)
        .max_size(MAX_BYTES as i64);
    for path in &target.paths {
        options.pathspec(path);
    }
    if target.paths.is_empty() {
        return Err(git2::Error::from_str("File path is unavailable"));
    }
    let diff = if let Kind::Commit(oid) = target.kind {
        let commit = repo.find_commit(oid)?;
        let tree = commit.tree()?;
        let parent = if commit.parent_count() > 0 {
            Some(commit.parent(0)?.tree()?)
        } else {
            None
        };
        repo.diff_tree_to_tree(parent.as_ref(), Some(&tree), Some(&mut options))?
    } else if target.kind == Kind::Staged {
        let tree = match repo.head() {
            Ok(head) => Some(head.peel_to_tree()?),
            Err(e)
                if matches!(
                    e.code(),
                    git2::ErrorCode::UnbornBranch | git2::ErrorCode::NotFound
                ) =>
            {
                None
            }
            Err(e) => return Err(e),
        };
        repo.diff_tree_to_index(tree.as_ref(), None, Some(&mut options))?
    } else {
        options
            .include_untracked(true)
            .show_untracked_content(true)
            .recurse_untracked_dirs(true);
        repo.diff_index_to_workdir(None, Some(&mut options))?
    };
    let mut lines = Vec::new();
    let mut bytes = 0;
    let mut truncated = false;
    let result = diff.print(DiffFormat::Patch, |_, _, line| {
        if bytes + line.content().len() > MAX_BYTES || lines.len() >= MAX_LINES {
            truncated = true;
            return false;
        }
        bytes += line.content().len();
        if line.origin() == 'B' {
            lines.push(
                "Binary file or file exceeds the 256 KiB text-preview limit; contents omitted."
                    .into(),
            );
            return true;
        }
        let prefix = match line.origin() {
            '+' => "+",
            '-' => "-",
            ' ' => " ",
            _ => "",
        };
        for text in String::from_utf8_lossy(line.content()).split_terminator('\n') {
            lines.push(format!("{prefix}{text}"));
        }
        true
    });
    if !truncated {
        result?;
    }
    if truncated {
        lines.push("… Preview limited to 256 KiB / 4,000 lines.".into());
    }
    if lines.is_empty() {
        lines.push("No text diff available (file changed, binary, or submodule).".into());
    }
    Ok(Preview { lines })
}
