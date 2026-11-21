use super::safe;
use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
};

pub fn render(frame: &mut Frame<'_>, app: &mut App, area: Rect) {
    let no_color = app.no_color;
    let view = app.inspection.as_mut().unwrap();
    let Some(result) = &view.result else {
        frame.render_widget(Paragraph::new("Loading commit…\nEsc back"), area);
        return;
    };
    let inspection = match result {
        Ok(inspection) => inspection,
        Err(error) => {
            frame.render_widget(
                Paragraph::new(format!("Cannot inspect commit: {}\nEsc back", safe(error))),
                area,
            );
            return;
        }
    };
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Percentage(30),
        Constraint::Min(2),
        Constraint::Length(1),
    ])
    .split(area);
    let comparison = if inspection.parents.is_empty() {
        "root commit · compared with empty tree"
    } else if inspection.parents.len() > 1 {
        "merge commit → compared with FIRST parent"
    } else {
        "compared with parent"
    };
    let date = chrono::DateTime::from_timestamp(inspection.timestamp, 0)
        .map(|t| t.to_rfc3339())
        .unwrap_or_default();
    frame.render_widget(
        Paragraph::new(format!(
            "Commit {}\n{} · {}\n{} · {} files{}",
            inspection.oid,
            safe(&inspection.author),
            date,
            comparison,
            inspection.files.len(),
            if inspection.truncated {
                " (list limited to 5,000)"
            } else {
                ""
            }
        )),
        rows[0],
    );
    let active = if no_color {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan)
    };
    let message: Vec<_> = inspection
        .message
        .lines()
        .map(|s| Line::from(safe(s)))
        .collect();
    view.max_message_scroll = message
        .len()
        .saturating_sub(rows[1].height.saturating_sub(2) as usize);
    view.message_scroll = view.message_scroll.min(view.max_message_scroll);
    frame.render_widget(
        Paragraph::new(message)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Message · Tab to focus ")
                    .border_style(if view.message_focus {
                        active
                    } else {
                        Style::default()
                    }),
            )
            .scroll((view.message_scroll.min(u16::MAX as usize) as u16, 0)),
        rows[1],
    );
    let height = rows[2].height.saturating_sub(2) as usize;
    if view.selected < view.scroll {
        view.scroll = view.selected;
    }
    if view.selected >= view.scroll + height {
        view.scroll = view.selected.saturating_add(1).saturating_sub(height);
    }
    let lines: Vec<_> = if inspection.files.is_empty() {
        vec![Line::from("No changed files (empty commit)")]
    } else {
        inspection
            .files
            .iter()
            .enumerate()
            .map(|(i, file)| {
                Line::styled(
                    format!(
                        "{} {}",
                        if i == view.selected { "▶" } else { " " },
                        safe(&file.path)
                    ),
                    if i == view.selected {
                        active.add_modifier(Modifier::REVERSED)
                    } else {
                        Style::default()
                    },
                )
            })
            .collect()
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Changed files · Enter opens diff ")
                    .border_style(if !view.message_focus {
                        active
                    } else {
                        Style::default()
                    }),
            )
            .scroll((view.scroll.min(u16::MAX as usize) as u16, 0)),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(
            "Esc back · ↑↓ select/scroll · Enter diff · Tab files/message · PgUp/PgDn · g/G",
        ),
        rows[3],
    );
}
