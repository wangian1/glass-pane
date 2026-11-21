use super::{RED, safe};
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
    let view = app.diff.as_mut().unwrap();
    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(format!(
            "{}\n{}",
            view.target.kind.label(),
            safe(&view.target.path)
        )),
        rows[0],
    );
    let texts = match &view.result {
        None => vec!["Loading diff…".to_string()],
        Some(Err(error)) => vec![format!("Cannot load diff: {}", safe(error))],
        Some(Ok(preview)) => preview.lines.clone(),
    };
    view.max_scroll = texts
        .len()
        .saturating_sub(rows[1].height.saturating_sub(2) as usize);
    view.scroll = view.scroll.min(view.max_scroll);
    let lines: Vec<_> = texts
        .into_iter()
        .map(|text| {
            let mut style = Style::default();
            if !no_color {
                style = style.fg(
                    if text.starts_with("+++")
                        || text.starts_with("---")
                        || text.starts_with("diff ")
                    {
                        Color::Cyan
                    } else if text.starts_with('+') {
                        Color::Green
                    } else if text.starts_with('-') {
                        RED
                    } else if text.starts_with("@@") {
                        Color::Cyan
                    } else {
                        Color::Reset
                    },
                );
            }
            if text.starts_with("@@") {
                style = style.add_modifier(Modifier::BOLD);
            }
            // Tabs expand predictably; other controls (including escape sequences) are neutralized.
            Line::styled(safe(&text.replace('\t', "    ")), style)
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(" Diff "))
            .scroll((view.scroll.min(u16::MAX as usize) as u16, view.horizontal)),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "Esc back · ↑↓ scroll · PgUp/PgDn · ←→ pan · r refresh  {}/{}",
            view.scroll + 1,
            view.max_scroll + 1
        )),
        rows[2],
    );
}
