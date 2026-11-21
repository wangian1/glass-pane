use super::{RED, clip, safe, style};
use crate::{actions::amend::Mode, app::App};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier},
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let Some(review) = &app.amend else {
        return;
    };
    let width = area.width.saturating_sub(2).min(94);
    let height = area.height.saturating_sub(2).min(30);
    let popup = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(style(app, Color::Cyan))
        .title(" Amend latest commit ");
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if area.width < 60 || area.height < 18 {
        frame.render_widget(Paragraph::new("Enlarge terminal to review amendment (60×18).\nEsc cancels; nothing has been changed.").wrap(Wrap { trim: false }), inner);
        return;
    }
    let parts = Layout::vertical([
        Constraint::Length(4),
        Constraint::Length(4),
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(inner);
    let plan = &review.plan;
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(clip(
                &format!("Branch: {}", safe(&plan.branch)),
                inner.width as usize,
            )),
            Line::from(clip(
                &format!(
                    "Commit: {} {}",
                    plan.commit.hash,
                    safe(&plan.commit.subject)
                ),
                inner.width as usize,
            )),
            Line::from(format!(
                "{} staged files · {} unstaged/untracked entries excluded",
                plan.staged.len(),
                plan.excluded_count
            )),
        ]),
        parts[0],
    );
    let options: Vec<_> = Mode::ALL
        .into_iter()
        .enumerate()
        .map(|(i, mode)| {
            let enabled = !mode.includes_staged() || !plan.staged.is_empty();
            let mut text_style = style(app, if enabled { Color::White } else { Color::Gray });
            if mode == review.mode {
                text_style =
                    style(app, Color::Cyan).add_modifier(Modifier::REVERSED | Modifier::BOLD);
            }
            Line::styled(
                format!(
                    "{} [{}] {}{}",
                    if mode == review.mode { "›" } else { " " },
                    i + 1,
                    mode.label(),
                    if enabled { "" } else { " (stage files first)" }
                ),
                text_style,
            )
        })
        .collect();
    frame.render_widget(Paragraph::new(options), parts[1]);
    frame.render_widget(
        Paragraph::new("Rewrites commit hash. If pushed, coordinate first. Never pushes.")
            .style(style(app, RED))
            .wrap(Wrap { trim: false }),
        parts[2],
    );
    let title = if review.mode.includes_staged() {
        "Files included · PgUp/PgDn"
    } else {
        "Files EXCLUDED; remain staged · PgUp/PgDn"
    };
    let files: Vec<_> = if plan.staged.is_empty() {
        vec![Line::from("No staged files")]
    } else {
        plan.staged
            .iter()
            .map(|f| {
                Line::from(clip(
                    &format!("{} {}", f.code, safe(&f.path)),
                    inner.width.saturating_sub(2) as usize,
                ))
            })
            .collect()
    };
    frame.render_widget(
        Paragraph::new(files)
            .block(Block::default().borders(Borders::TOP).title(title))
            .scroll((
                review
                    .file_scroll
                    .min(
                        plan.staged
                            .len()
                            .saturating_sub(parts[3].height.saturating_sub(1) as usize),
                    )
                    .min(u16::MAX as usize) as u16,
                0,
            )),
        parts[3],
    );
    frame.render_widget(
        Paragraph::new(
            "1/2/3 or ↑/↓: choose · Enter: confirm amend\nPgUp/PgDn: review files · Esc: cancel",
        )
        .style(style(app, Color::Cyan)),
        parts[4],
    );
}
