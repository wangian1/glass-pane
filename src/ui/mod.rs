mod amend;
mod branches;
mod commit;
mod commits;
mod diff;
mod footer;
mod graph;
mod header;
mod help;
mod inspect;
mod staging;

use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

// Bright coral stays legible on dark terminal backgrounds.
const RED: Color = Color::Rgb(255, 128, 128);

// Truncate at grapheme boundaries, keeping CJK and emoji within the cell budget.
pub(crate) fn clip(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut used = 0;
    for grapheme in text.graphemes(true) {
        let size = grapheme.width();
        if used + size > width - 1 {
            break;
        }
        used += size;
        result.push_str(grapheme);
    }
    result.push('…');
    result
}

pub fn safe(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { '�' } else { c })
        .collect()
}

pub fn style(app: &App, color: Color) -> Style {
    if app.no_color {
        Style::default()
    } else {
        Style::default().fg(color)
    }
}

pub fn relative(timestamp: i64) -> String {
    let seconds = chrono::Utc::now()
        .timestamp()
        .saturating_sub(timestamp)
        .max(0);
    match seconds {
        0..60 => "now".into(),
        60..3600 => format!("{}m ago", seconds / 60),
        3600..86400 => format!("{}h ago", seconds / 3600),
        _ => format!("{}d ago", seconds / 86400),
    }
}

fn panel(
    frame: &mut Frame<'_>,
    app: &mut App,
    index: usize,
    area: Rect,
    title: String,
    mut lines: Vec<Line<'static>>,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    if lines.is_empty() {
        lines.push(Line::from("  None"));
    }
    let height = area.height.saturating_sub(2) as usize;
    app.max_scroll[index] = lines.len().saturating_sub(height).min(u16::MAX as usize);
    app.scroll[index] = app.scroll[index].min(app.max_scroll[index]);
    if height > 0
        && let Some(selected) = match index {
            1 => app.selected_branch_row(),
            2 => app.selected_commit_row(),
            3 => app.selected_file_row(),
            _ => None,
        }
    {
        if selected < app.scroll[index] {
            app.scroll[index] = selected;
        }
        if selected >= app.scroll[index] + height {
            app.scroll[index] = selected + 1 - height;
        }
        app.scroll[index] = app.scroll[index].min(app.max_scroll[index]);
    }
    let focused = app.focus == index;
    let border_style = if focused {
        style(app, Color::Cyan).add_modifier(Modifier::BOLD)
    } else {
        style(app, Color::DarkGray)
    };
    let indicator = if focused { "▶ " } else { "" };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(format!(" {indicator}{title} "));
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .scroll((app.scroll[index] as u16, 0)),
        area,
    );
}

pub fn render(frame: &mut Frame<'_>, app: &mut App) {
    let area = frame.area();
    if app.diff.is_some() {
        diff::render(frame, app, area);
        return;
    }
    if app.inspection.is_some() {
        inspect::render(frame, app, area);
        return;
    }
    if area.width < 30 || area.height < 12 {
        frame.render_widget(
            Paragraph::new("glasspane — enlarge terminal (30×12 minimum)\n[q] quit"),
            area,
        );
        return;
    }
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    header::render(frame, app, layout[0]);
    footer::render(frame, app, layout[2]);
    if let Some(error) = &app.error {
        frame.render_widget(
            Paragraph::new(format!(
                "{}\n\nWaiting for a directory change. Press r to retry, ? for help, q to quit.",
                safe(error)
            ))
            .block(Block::default().borders(Borders::ALL).title(" Repository ")),
            layout[1],
        );
    } else if app.snapshot.is_none() {
        frame.render_widget(Paragraph::new("Loading repository…"), layout[1]);
    } else {
        let body = layout[1];
        // Wide terminals show the brief's grid; narrow ones show a focused full-width panel.
        let snapshot = app.snapshot.as_ref().unwrap();
        let titles = [
            "Branch Graph".to_string(),
            "Branches".to_string(),
            format!("Recent Commits · {}", safe(&snapshot.head)),
            "Staging".to_string(),
        ];
        let grid = body.width >= 100 && body.height >= 14 && !app.maximized;
        let areas = if grid {
            let rows = Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(body);
            let top = Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(rows[0]);
            let bottom =
                Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                    .split(rows[1]);
            [top[0], top[1], bottom[0], bottom[1]]
        } else {
            [body; 4]
        };
        let content = [
            graph::lines(app, areas[0].width.saturating_sub(2)),
            branches::lines(app),
            commits::lines(app),
            staging::lines(app),
        ];
        for (i, (title, lines)) in titles.into_iter().zip(content).enumerate() {
            if grid || i == app.focus {
                let hint = if i == 3 {
                    "Enter diff · Space expand"
                } else if i == 2 {
                    "Enter inspect · Space expand"
                } else if i == 1 {
                    "Enter switch · Space expand"
                } else if app.maximized {
                    "Enter restore"
                } else if grid {
                    "Enter expand"
                } else {
                    "Tab switch · Enter expand"
                };
                panel(frame, app, i, areas[i], format!("{title} · {hint}"), lines);
            }
        }
    }
    if app.help {
        help::render(frame, app, area);
    }
    if app.commit.is_some() {
        commit::render(frame, app, area);
    }
    if app.amend.is_some() {
        amend::render(frame, app, area);
    }
}
