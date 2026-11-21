use super::{safe, style};
use crate::app::App;
use ratatui::{
    Frame,
    layout::Rect,
    style::Color,
    text::{Line, Span},
    widgets::Paragraph,
};

pub fn render(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if let Some(message) = &app.action_message {
        frame.render_widget(
            Paragraph::new(safe(message)).style(style(
                app,
                if app.switching || app.amend_preparing || app.staging_busy || app.commit_preparing
                {
                    Color::Yellow
                } else {
                    Color::Green
                },
            )),
            area,
        );
        return;
    }
    let status = if app.polling {
        "○ polling"
    } else {
        "● live"
    };
    let keys = if app.focus == 3 {
        "  [j/k] file [s/u] stage/unstage [Enter] diff [!] shell [?] help [q] quit"
    } else if app.focus == 2 {
        "  [j/k] commit [Enter] inspect [c] commit [Space] expand [?] help [q] quit"
    } else if app.focus == 1 {
        "  [j/k] select [Enter] switch [c] commit [a] amend [Space] expand [?] help [q] quit"
    } else if area.width < 45 {
        "  [c] commit [a] amend [!] shell [?] help [q] quit"
    } else if area.width < 90 {
        "  [Tab] panel [Enter] expand [c] commit [a] amend [!] shell [?] help [q] quit"
    } else {
        "  [Tab] panel [Enter] expand [c] commit [a] amend [!] shell [j/k] scroll [r] refresh [?] help [q] quit"
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                status,
                style(
                    app,
                    if app.polling {
                        Color::Yellow
                    } else {
                        Color::Green
                    },
                ),
            ),
            Span::raw(keys),
        ])),
        area,
    );
}
