//! Reviewed commits use native Git for hooks, signing and commit semantics.
use crate::git::{self, status::FileStatus};
use anyhow::{Context, Result, bail};
use git2::{Oid, Repository, RepositoryState};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::ExitStatus,
};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, PartialEq, Eq)]
struct State {
    head: Option<Oid>,
    reference: Vec<u8>,
    index: Vec<(Vec<u8>, Oid, u32, u16, u16)>,
}
fn state(repo: &Repository) -> Result<State> {
    let reference = repo.find_reference("HEAD")?;
    let head = match repo.head() {
        Ok(head) => Some(head.peel_to_commit()?.id()),
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch => None,
        Err(e) => return Err(e.into()),
    };
    Ok(State {
        head,
        reference: reference
            .symbolic_target_bytes()
            .unwrap_or(reference.name_bytes())
            .to_vec(),
        index: repo
            .index()?
            .iter()
            .map(|e| (e.path, e.id, e.mode, e.flags, e.flags_extended))
            .collect(),
    })
}
#[derive(Debug)]
pub struct Plan {
    pub root: PathBuf,
    pub branch: String,
    pub staged: Vec<FileStatus>,
    pub excluded_count: usize,
    state: State,
}
pub fn prepare(root: &Path) -> Result<Plan> {
    let repo = Repository::open(root)?;
    if repo.is_bare() {
        bail!("Committing requires a working tree.");
    }
    if repo.state() != RepositoryState::Clean {
        bail!("Finish the current merge, rebase or other Git operation before committing here.");
    }
    let before = state(&repo)?;
    let staging = git::status::load(&repo)?;
    if !staging.conflicted.is_empty() {
        bail!("Resolve conflicts before committing.");
    }
    if staging.staged.is_empty() {
        bail!("No staged changes. Select files in Staging and press s first.");
    }
    let after = state(&Repository::open(root)?)?;
    if before != after {
        bail!("Repository changed while preparing commit. Open the dialog again.");
    }
    Ok(Plan {
        root: repo.workdir().unwrap().into(),
        branch: git::head(&repo)?.0,
        staged: staging.staged,
        excluded_count: staging.unstaged.len() + staging.untracked.len(),
        state: before,
    })
}
pub fn validate(plan: &Plan, message: &str) -> Result<()> {
    if message.trim().is_empty() {
        bail!("Enter a commit message.");
    }
    if message.contains('\0') {
        bail!("Commit message contains a NUL character.");
    }
    let current = prepare(&plan.root)?;
    if current.state != plan.state {
        bail!(
            "HEAD, branch or staged contents changed after review. Cancel and open a fresh commit dialog."
        );
    }
    Ok(())
}
pub fn finish(plan: &Plan, status: ExitStatus) -> Result<String> {
    let repo = Repository::open(&plan.root)?;
    let head = state(&repo)?.head;
    if head != plan.state.head {
        let hash = head
            .context("HEAD disappeared while committing")?
            .to_string()[..7]
            .to_string();
        return Ok(if status.success() {
            format!("Committed {hash}")
        } else {
            format!(
                "Created commit {hash}; Git reported {status}. Review hook output in the terminal."
            )
        });
    }
    bail!(
        "Commit did not complete ({status}); HEAD is unchanged. See Git output in the normal terminal."
    )
}
pub fn execute(plan: &Plan, message: &str) -> Result<String> {
    validate(plan, message)?;
    // Keep arbitrary message text out of command arguments and shell parsing.
    let mut file = tempfile::NamedTempFile::new()?;
    file.write_all(message.as_bytes())?;
    file.flush()?;
    let mut command = super::git_command(&plan.root);
    command
        .args(["commit", "--cleanup=strip", "--file"])
        .arg(file.path());
    crate::shell::reset_child_interrupt(&mut command);
    let status = command.status().context("Cannot run git commit")?;
    finish(plan, status)
}

#[derive(Debug, Default)]
pub struct Editor {
    pub text: String,
    pub cursor: usize,
}
impl Editor {
    pub fn insert(&mut self, value: &str) {
        // Avoid control sequences in messages; tabs/newlines are useful text.
        let value: String = value
            .chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
            .collect();
        if self.text.len() + value.len() > 64 * 1024 {
            return;
        }
        self.text.insert_str(self.cursor, &value);
        self.cursor += value.len();
    }
    pub fn left(&mut self) {
        self.cursor = self.text[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0);
    }
    pub fn right(&mut self) {
        self.cursor += self.text[self.cursor..]
            .graphemes(true)
            .next()
            .map(str::len)
            .unwrap_or(0);
    }
    pub fn backspace(&mut self) {
        let end = self.cursor;
        self.left();
        self.text.replace_range(self.cursor..end, "");
    }
    pub fn delete(&mut self) {
        let start = self.cursor;
        self.right();
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }
    pub fn home(&mut self) {
        self.cursor = self.text[..self.cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
    }
    pub fn end(&mut self) {
        self.cursor += self.text[self.cursor..]
            .find('\n')
            .unwrap_or(self.text.len() - self.cursor);
    }
    pub fn vertical(&mut self, down: bool) {
        let start = self.text[..self.cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let column = self.text[start..self.cursor].graphemes(true).count();
        let next = if down {
            match self.text[self.cursor..].find('\n') {
                Some(i) => self.cursor + i + 1,
                None => return,
            }
        } else {
            if start == 0 {
                return;
            }
            self.text[..start - 1]
                .rfind('\n')
                .map(|i| i + 1)
                .unwrap_or(0)
        };
        let end = next
            + self.text[next..]
                .find('\n')
                .unwrap_or(self.text.len() - next);
        self.cursor = self.text[next..end]
            .grapheme_indices(true)
            .nth(column)
            .map(|(i, _)| next + i)
            .unwrap_or(end);
    }
}
#[derive(Debug)]
pub struct Review {
    pub plan: Plan,
    pub editor: Editor,
    pub error: Option<String>,
    pub file_scroll: usize,
}
impl Review {
    pub fn new(plan: Plan) -> Self {
        Self {
            plan,
            editor: Editor::default(),
            error: None,
            file_scroll: 0,
        }
    }
}
