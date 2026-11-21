use super::safe;
use crate::app::App;
use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let width = area.width.min(68);
    let height = area.height.min(23);
    let popup = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let mut text = "Tab / Shift+Tab   Next / previous panel\nj / k, ↓ / ↑       Select branch, commit or file / scroll\ng / G              First / last branch or panel bounds\nEnter              Switch branch / inspect commit / file diff\nSpace              Expand / restore any panel\ns / u              Stage / unstage selected file\nr                  Refresh everything\nc                  Review staged files and commit\na                  Review amendment of latest commit\n!                  Open shell (exit / Ctrl+D returns)\n? / Esc            Close help (Esc also restores panel)\nq / Ctrl+C         Quit\n\nBranch Enter: safe checkout · no automatic fetch\nNarrow terminals show the focused panel.\nRemote refs reflect the last manual fetch.\nDirectory tracking: last shell writer wins.".to_string();
    if let Some(error) = &app.action_error {
        text = format!(
            "Action failed: {}\n\nEsc closes this message.\n\n{text}",
            safe(&error.replace('\n', " "))
        );
    }
    if let Some(error) = &app.shell_error {
        text = format!("Shell error: {}\n\n{text}", safe(error));
    }
    if let Some(error) = &app.watcher_error {
        text.push_str(&format!(
            "\n\nWatcher error (polling every 2s): {}",
            safe(error)
        ));
    }
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(" Help ")),
        popup,
    );
}
