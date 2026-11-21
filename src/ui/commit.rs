use super::{clip, safe, style};
use crate::app::App;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let review = app.commit.as_ref().unwrap();
    let width = area.width.saturating_sub(2).min(100);
    let height = area.height.saturating_sub(2).min(36);
    let popup = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Create commit ")
        .border_style(style(app, Color::Cyan));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    if area.width < 60 || area.height < 20 {
        frame.render_widget(Paragraph::new("Enlarge terminal to review commit (60×20).\nEsc cancels; nothing has been committed."), inner);
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(5),
        Constraint::Min(3),
        Constraint::Length(2),
        Constraint::Length(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(clip(
                &format!("Branch: {}", safe(&review.plan.branch)),
                inner.width as usize,
            )),
            Line::from(format!(
                "{} staged files · {} unstaged/untracked entries excluded",
                review.plan.staged.len(),
                review.plan.excluded_count
            )),
        ]),
        rows[0],
    );
    let files: Vec<_> = review
        .plan
        .staged
        .iter()
        .map(|f| Line::from(format!("{} {}", f.code, safe(&f.path))))
        .collect();
    frame.render_widget(
        Paragraph::new(files)
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .title(" Included files · PgUp/PgDn "),
            )
            .scroll((
                review
                    .file_scroll
                    .min(review.plan.staged.len().saturating_sub(4))
                    .min(u16::MAX as usize) as u16,
                0,
            )),
        rows[1],
    );
    let before = &review.editor.text[..review.editor.cursor];
    let cursor_row = before.chars().filter(|&c| c == '\n').count();
    let cursor_column = before
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .replace('\t', "    ")
        .width();
    let visible_height = rows[2].height.saturating_sub(2).max(1) as usize;
    let scroll = cursor_row.saturating_sub(visible_height - 1);
    let horizontal = cursor_column.saturating_sub(rows[2].width.saturating_sub(4) as usize);
    let mut offset = 0;
    let mut lines = vec![];
    for line in review.editor.text.split('\n') {
        let mut spans = vec![];
        for (i, ch) in line.grapheme_indices(true) {
            let text = ch.replace('\t', "    ");
            spans.push(Span::styled(
                text,
                if offset + i == review.editor.cursor {
                    style(app, Color::Cyan).add_modifier(Modifier::REVERSED)
                } else {
                    ratatui::style::Style::default()
                },
            ));
        }
        if offset + line.len() == review.editor.cursor {
            spans.push(Span::styled(
                " ",
                style(app, Color::Cyan).add_modifier(Modifier::REVERSED),
            ));
        }
        lines.push(Line::from(spans));
        offset += line.len() + 1;
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Message · Enter adds a line "),
            )
            .scroll((
                scroll.min(u16::MAX as usize) as u16,
                horizontal.min(u16::MAX as usize) as u16,
            )),
        rows[2],
    );
    if let Some(error) = &review.error {
        frame.render_widget(
            Paragraph::new(safe(error)).style(style(app, super::RED)),
            rows[3],
        );
    }
    frame.render_widget(
        Paragraph::new("Ctrl+S: commit staged files · Esc: cancel").style(style(app, Color::Cyan)),
        rows[4],
    );
}
