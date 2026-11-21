use super::{RED, clip, safe, style};
use crate::{
    app::App,
    git::graph::{GraphCommit, GraphLine, LaneColor},
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthStr;

fn lane_color(color: Option<LaneColor>) -> Color {
    match color {
        Some(LaneColor::Cyan) | None => Color::Cyan,
        Some(LaneColor::Magenta) => Color::Magenta,
        Some(LaneColor::Yellow) => Color::Yellow,
        Some(LaneColor::Blue) => Color::LightBlue,
        Some(LaneColor::Green) => Color::Green,
        Some(LaneColor::Red) => RED,
    }
}

fn badge_style(app: &App, name: &str) -> Style {
    if name == "HEAD" || name.starts_with("HEAD -> ") {
        style(app, Color::Green).add_modifier(Modifier::BOLD)
    } else if name.starts_with("tag: ") {
        style(app, Color::Yellow)
    } else if app
        .snapshot
        .as_ref()
        .is_some_and(|s| s.remote_branches.iter().any(|b| b.name == name))
    {
        style(app, RED)
    } else {
        style(app, Color::Cyan)
    }
}

fn badges(app: &App, commit: &GraphCommit, width: usize) -> Vec<Span<'static>> {
    let mut remaining = width;
    let mut spans = vec![];
    for name in &commit.decorations {
        if remaining < 3 {
            break;
        }
        let label = safe(&name.replace("HEAD -> ", "HEAD → "));
        let label = format!("[{}]", clip(&label, remaining - 2));
        remaining -= label.width();
        spans.push(Span::styled(label, badge_style(app, name)));
        if remaining > 0 {
            spans.push(Span::raw(" "));
            remaining -= 1;
        }
    }
    spans
}

fn row(app: &App, row: &GraphLine, gutter: usize, width: usize) -> Line<'static> {
    let head = row.commit.as_ref().is_some_and(GraphCommit::is_head);
    let mut spans = vec![];
    for cell in &row.lanes {
        let symbol = match cell.symbol {
            '*' => '●',
            '|' => '│',
            '/' => '╱',
            '\\' => '╲',
            '-' | '_' => '─',
            // Octopus merge endpoints stay recognizable; do not invent topology.
            '.' => '┐',
            other => other,
        };
        let mut text_style = style(app, lane_color(cell.color));
        if cell.symbol == '*' {
            text_style = style(
                app,
                if head {
                    Color::Green
                } else {
                    lane_color(cell.color)
                },
            )
            .add_modifier(Modifier::BOLD);
        }
        spans.push(Span::styled(symbol.to_string(), text_style));
    }
    let Some(commit) = &row.commit else {
        return Line::from(spans);
    };
    spans.push(Span::raw(
        " ".repeat(gutter.saturating_sub(row.lanes.len())),
    ));
    let available = width.saturating_sub(gutter);
    let hash_width = available.min(8);
    let hash = clip(&safe(&commit.hash), hash_width.saturating_sub(1));
    spans.push(Span::styled(
        format!("{hash:<hash_width$}"),
        style(app, Color::Gray).add_modifier(Modifier::DIM),
    ));
    let available = available.saturating_sub(hash_width);
    let labels_width: usize = commit
        .decorations
        .iter()
        .map(|s| safe(&s.replace("HEAD -> ", "HEAD → ")).width() + 3)
        .sum();
    // Keep messages aligned and give refs a separate right-hand column. Long
    // labels never consume the entire commit message; Enter offers more space.
    let label_width = if available >= 16 {
        labels_width.min(available / 2)
    } else {
        0
    };
    let gap = if label_width > 0 { 2 } else { 0 };
    let subject_width = available.saturating_sub(label_width + gap);
    let subject = clip(&safe(&commit.subject), subject_width);
    let subject_style = if head {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    spans.push(Span::styled(subject.clone(), subject_style));
    if label_width > 0 {
        spans.push(Span::raw(
            " ".repeat(subject_width.saturating_sub(subject.width()) + gap),
        ));
        spans.extend(badges(app, commit, label_width));
    }
    Line::from(spans)
}

pub fn lines(app: &App, width: u16) -> Vec<Line<'static>> {
    match &app.snapshot.as_ref().unwrap().graph {
        Ok(rows) if rows.is_empty() => vec![Line::from("  No commits yet")],
        Ok(rows) => {
            let gutter = rows.iter().map(|r| r.lanes.len()).max().unwrap_or(0) + 1;
            rows.iter()
                .map(|r| row(app, r, gutter, width as usize))
                .collect()
        }
        Err(error) => vec![Line::from(safe(error))],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::graph::parse;

    fn plain(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn unicode_graph_aligns_subjects_and_keeps_head_ref_visible() {
        let app = App::new("/repo".into());
        let rows = parse(
            "*   \x1fabc1234\x1fHEAD -> main\x1fMerge topic\n|\\  \n| * \x1fdef5678\x1forigin/topic\x1fTopic\n",
        );
        let rendered: Vec<_> = rows.iter().map(|r| row(&app, r, 5, 80)).collect();
        let first = plain(&rendered[0]);
        let third = plain(&rendered[2]);
        assert!(first.starts_with("●    abc1234 Merge topic"));
        assert!(first.contains("[HEAD → main]"));
        assert_eq!(
            first[..first.find("Merge topic").unwrap()].width(),
            third[..third.find("Topic").unwrap()].width()
        );
        assert_eq!(plain(&rendered[1]), "│╲  ");
        assert!(rendered.iter().all(|r| r.width() <= 80));
    }

    #[test]
    fn long_refs_and_wide_unicode_subjects_fit_narrow_panels() {
        let app = App::new("/repo".into());
        let rows = parse(
            "* \x1fabc1234\x1fHEAD -> feature/very-long-branch\x1f修正 👩‍💻 café with a long subject",
        );
        for width in [24, 30, 58, 80] {
            let rendered = row(&app, &rows[0], 3, width);
            assert!(rendered.width() <= width);
            assert!(plain(&rendered).contains('●'));
        }
        assert_eq!(clip("👩‍💻 coding", 4), "👩‍💻 …");
    }

    #[test]
    fn no_color_disables_lane_and_badge_colors() {
        let mut app = App::new("/repo".into());
        app.no_color = true;
        let rows = parse("\x1b[35m*\x1b[m \x1fabc1234\x1fHEAD -> main\x1fSubject");
        let rendered = row(&app, &rows[0], 3, 80);
        assert!(
            rendered
                .spans
                .iter()
                .all(|s| s.style.fg.is_none() && s.style.bg.is_none())
        );
        assert!(!plain(&rendered).contains('\x1b'));
    }
}
