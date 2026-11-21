//! A small, read-only dashboard printed into ordinary terminal scrollback.
use crate::{
    git::{self, log::CommitInfo, status::Staging},
    ui::{clip, safe},
};
use crossterm::{
    queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
};
use std::{
    io::{self, IsTerminal, Write},
    path::Path,
};
use unicode_width::UnicodeWidthStr;

#[derive(Debug)]
pub struct Summary {
    pub head: String,
    pub latest: Option<CommitInfo>,
    pub state: &'static str,
    pub staging: Staging,
}

pub fn load(path: &Path) -> Result<Summary, git::LoadError> {
    let repo = git2::Repository::discover(path).map_err(|error| {
        if error.code() == git2::ErrorCode::NotFound {
            git::LoadError::NotRepository(path.to_path_buf())
        } else {
            git::LoadError::Git(error)
        }
    })?;
    let (head, latest) = git::head(&repo)?;
    Ok(Summary {
        head,
        latest,
        state: git::state::load(&repo),
        staging: git::status::load(&repo)?,
    })
}

#[derive(Debug)]
pub struct SummaryLine {
    pub text: String,
    pub color: Option<Color>,
}

pub fn render(summary: &Summary, width: usize) -> Vec<SummaryLine> {
    let width = width.min(96);
    let boxed = width >= 24;
    let content_width = if boxed { width - 4 } else { width };
    let s = &summary.staging;
    let mut rows = vec![
        (
            format!(
                "Branch  {}{}",
                summary.head,
                if summary.state.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", summary.state)
                }
            ),
            Some(Color::Green),
        ),
        (
            format!(
                "Commit  {}",
                summary
                    .latest
                    .as_ref()
                    .map(|c| format!("{} {}", c.hash, c.subject))
                    .unwrap_or_else(|| "No commits yet".into())
            ),
            None,
        ),
    ];
    let conflicted = Color::Rgb {
        r: 255,
        g: 128,
        b: 128,
    };
    let staging_text = format!(
        "Staging {} staged · {} unstaged · {} untracked · {} conflicted",
        s.staged.len(),
        s.unstaged.len(),
        s.untracked.len(),
        s.conflicted.len()
    );
    if width >= 64 && staging_text.width() <= content_width {
        let clean = s.staged.is_empty()
            && s.unstaged.is_empty()
            && s.untracked.is_empty()
            && s.conflicted.is_empty();
        let staging_text = if clean && staging_text.width() + " · clean".width() <= content_width {
            format!("{staging_text} · clean")
        } else {
            staging_text
        };
        rows.push((
            staging_text,
            Some(if !s.conflicted.is_empty() {
                conflicted
            } else if !s.unstaged.is_empty() {
                Color::Yellow
            } else {
                Color::Green
            }),
        ));
    } else {
        for (name, count, color) in [
            ("Staged", s.staged.len(), Color::Green),
            ("Unstaged", s.unstaged.len(), Color::Yellow),
            ("Untracked", s.untracked.len(), Color::Grey),
            ("Conflicts", s.conflicted.len(), conflicted),
        ] {
            rows.push((format!("{name:<10}{count}"), Some(color)));
        }
    }
    let mut result = vec![];
    if boxed {
        let title = "╭─ glasspane ";
        result.push(SummaryLine {
            text: format!("{title}{}╮", "─".repeat(width - title.width() - 1)),
            color: Some(Color::Cyan),
        });
    }
    for (text, color) in rows {
        let text = clip(&safe(&text), content_width);
        let text = if boxed {
            format!(
                "│ {text}{} │",
                " ".repeat(content_width.saturating_sub(text.width()))
            )
        } else {
            text
        };
        result.push(SummaryLine { text, color });
    }
    if boxed {
        result.push(SummaryLine {
            text: format!("╰{}╯", "─".repeat(width - 2)),
            color: Some(Color::Cyan),
        });
    }
    result
}

pub fn print(path: &Path) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    let colored = stdout.is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let width = crossterm::terminal::size()
        .map(|(w, _)| usize::from(w))
        .unwrap_or(80);
    match load(path) {
        Ok(summary) => {
            for line in render(&summary, width) {
                if colored && let Some(color) = line.color {
                    queue!(stdout, SetForegroundColor(color))?;
                }
                queue!(stdout, Print(line.text))?;
                if colored {
                    queue!(stdout, ResetColor)?;
                }
                writeln!(stdout)?;
            }
        }
        Err(error) => writeln!(
            stdout,
            "{}",
            clip(&safe(&format!("glasspane · {error}")), width)
        )?,
    }
    stdout.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::status::FileStatus;

    #[test]
    fn compact_summary_shows_partial_staging_and_latest_commit() {
        let file = FileStatus {
            paths: vec![],
            code: 'M',
            path: "file".into(),
        };
        let summary = Summary {
            head: "main".into(),
            latest: Some(CommitInfo {
                oid: git2::Oid::ZERO_SHA1,
                hash: "a1b2c3d".into(),
                subject: "Latest change".into(),
                author: "Test".into(),
                timestamp: 0,
            }),
            state: "",
            staging: Staging {
                staged: vec![file.clone()],
                unstaged: vec![file],
                ..Staging::default()
            },
        };
        let lines = render(&summary, 96);
        assert_eq!(lines.len(), 5);
        assert!(lines[1].text.contains("main"));
        assert!(lines[2].text.contains("a1b2c3d Latest change"));
        assert!(
            lines[3]
                .text
                .contains("1 staged · 1 unstaged · 0 untracked · 0 conflicted")
        );
        assert!(lines.iter().all(|l| l.text.width() == 96));
    }

    #[test]
    fn narrow_and_unicode_summaries_fit_and_escape_control_characters() {
        let summary = Summary {
            head: "feature/修正\x1b[31m".into(),
            latest: None,
            state: "MERGING",
            staging: Staging::default(),
        };
        for width in [0, 10, 24, 40, 64, 80, 120] {
            let lines = render(&summary, width);
            assert!(lines.iter().all(|l| l.text.width() <= width.min(96)));
            assert!(lines.iter().all(|l| !l.text.contains('\x1b')));
        }
        assert!(
            render(&summary, 40)
                .iter()
                .any(|l| l.text.contains("No commits yet"))
        );
    }
}
