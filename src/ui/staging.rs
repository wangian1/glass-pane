use super::{RED, safe, style};
use crate::{app::App, git::diff::Kind};
use ratatui::{
    style::{Color, Modifier},
    text::Line,
};

pub fn lines(app: &App) -> Vec<Line<'static>> {
    let s = &app.snapshot.as_ref().unwrap().staging;
    let mut rows = vec![];
    for (kind, name, files, color) in [
        (Kind::Conflicted, "CONFLICTED", &s.conflicted, RED),
        (Kind::Staged, "STAGED", &s.staged, Color::Green),
        (Kind::Unstaged, "UNSTAGED", &s.unstaged, Color::Yellow),
        (Kind::Untracked, "UNTRACKED", &s.untracked, Color::Gray),
    ] {
        if files.is_empty() {
            continue;
        }
        let text_style = style(app, color);
        rows.push(Line::styled(
            format!("{name} ({})", files.len()),
            text_style.add_modifier(Modifier::BOLD),
        ));
        for file in files {
            let selected = app
                .selected_file
                .as_ref()
                .is_some_and(|f| f.kind == kind && f.paths == file.paths && f.path == file.path);
            rows.push(Line::styled(
                format!(
                    "{} {} {}",
                    if selected { "▶" } else { " " },
                    file.code,
                    safe(&file.path)
                ),
                if selected {
                    text_style.add_modifier(Modifier::REVERSED | Modifier::BOLD)
                } else {
                    text_style
                },
            ));
        }
    }
    if rows.is_empty() {
        rows.push(Line::from("  Working tree clean"));
    }
    rows
}
