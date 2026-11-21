use super::{relative, safe, style};
use crate::app::App;
use ratatui::{
    style::{Color, Modifier},
    text::{Line, Span},
};

pub fn lines(app: &App) -> Vec<Line<'static>> {
    let commits = &app.snapshot.as_ref().unwrap().commits;
    if commits.is_empty() {
        return vec![Line::from("  No commits yet")];
    }
    commits
        .iter()
        .map(|c| {
            Line::from(vec![
                Span::styled(
                    format!(
                        "{} {}  ",
                        if app.selected_commit == Some(c.oid) {
                            "▶"
                        } else {
                            " "
                        },
                        c.hash
                    ),
                    if app.selected_commit == Some(c.oid) {
                        style(app, Color::Yellow).add_modifier(Modifier::REVERSED | Modifier::BOLD)
                    } else {
                        style(app, Color::Yellow)
                    },
                ),
                Span::raw(format!(
                    "{}  {}  {}",
                    safe(&c.author),
                    relative(c.timestamp),
                    safe(&c.subject)
                )),
            ])
        })
        .collect()
}
