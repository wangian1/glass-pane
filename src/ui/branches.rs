use super::{RED, relative, safe, style};
use crate::{actions::BranchTarget, app::App};
use ratatui::{
    style::{Color, Modifier},
    text::Line,
};

pub fn lines(app: &App) -> Vec<Line<'static>> {
    let s = app.snapshot.as_ref().unwrap();
    let mut rows = vec![Line::from("LOCAL")];
    if s.local_branches.is_empty() {
        rows.push(Line::from("  No local branches yet"));
    }
    for (branches, remote) in [(&s.local_branches, false), (&s.remote_branches, true)] {
        if remote {
            let fetched = s
                .last_fetch
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|t| relative(t.as_secs() as i64));
            rows.push(Line::from(format!(
                "REMOTE (as of last fetch{})",
                fetched.map(|s| format!(" · {s}")).unwrap_or_default()
            )));
            if branches.is_empty() {
                rows.push(Line::from("  No remote-tracking branches"));
            }
        }
        for branch in branches {
            let is_main = !remote && branch.name == "main";
            let color = if is_main {
                Color::Rgb(255, 215, 95)
            } else if branch.current {
                Color::Green
            } else if remote {
                RED
            } else {
                Color::White
            };
            let mut text_style = style(app, color);
            if branch.current || is_main {
                text_style = text_style.add_modifier(Modifier::BOLD);
            }
            let selected = app.selected_branch.as_ref()
                == Some(&BranchTarget {
                    name: branch.name.clone(),
                    remote,
                });
            if selected && app.focus == 1 {
                text_style = text_style.add_modifier(Modifier::REVERSED | Modifier::BOLD);
            }
            rows.push(Line::styled(
                format!(
                    "{} {} {}  {}  {}",
                    if selected { "›" } else { " " },
                    if branch.current { "●" } else { " " },
                    safe(&branch.name),
                    branch.hash,
                    relative(branch.timestamp)
                ),
                text_style,
            ));
        }
    }
    rows
}
