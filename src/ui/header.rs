use super::{RED, safe, style};
use crate::app::App;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let cwd = format!(
        "{}{}",
        safe(&app.cwd.to_string_lossy()),
        if app.following { "" } else { " (launch dir)" }
    );
    let rows = if let Some(s) = &app.snapshot {
        vec![
            Line::from(format!(
                " repo: {}  cwd: {}",
                safe(&s.root.to_string_lossy()),
                cwd
            )),
            Line::from(vec![
                Span::raw(" branch: "),
                Span::styled(
                    safe(&s.head),
                    style(app, Color::Green).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  {}", s.state),
                    style(app, RED).add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!(
                    "  upstream: {}",
                    safe(s.upstream.as_deref().unwrap_or("—"))
                )),
            ]),
            Line::from(format!(
                " HEAD: {}{}",
                s.head_commit
                    .as_ref()
                    .map(|c| format!("{} {}", c.hash, safe(&c.subject)))
                    .unwrap_or_else(|| "No commits yet".into()),
                if app.loading { "  refreshing…" } else { "" }
            )),
        ]
    } else {
        vec![
            Line::from(format!(" cwd: {cwd}")),
            Line::from(" branch: —"),
            Line::from(" HEAD: —"),
        ]
    };
    frame.render_widget(
        Paragraph::new(rows).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" glasspane · live Git dashboard "),
        ),
        area,
    );
}
