//! Rendering for the ALERTS dashboard. Browse mode shows today's alerts on the
//! left with a level/location stats panel on the right; Create mode shows a form
//! to raise an alert by hand.
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

use super::domain::{self, Alert, AlertLevel};
use super::handler::{AlertsMode, AlertsScreen, Draft, DraftField};
use crate::map::domain::MapRoom;
use crate::ui::chrome::panel;
use crate::ui::theme::{AMBER, PHOSPHOR, PHOSPHOR_DIM, PHOSPHOR_FAINT};

/// Bright red for ERROR-level alerts (the palette is otherwise green/amber).
const RED: Color = Color::Rgb(255, 80, 60);

/// Number of cells in a stat bar gauge.
const GAUGE: usize = 16;

fn level_color(level: AlertLevel) -> Color {
    match level {
        AlertLevel::Debug => PHOSPHOR_FAINT,
        AlertLevel::Info => PHOSPHOR,
        AlertLevel::Warn => AMBER,
        AlertLevel::Error => RED,
    }
}

pub fn render(f: &mut Frame, area: Rect, screen: &AlertsScreen, alerts: &[Alert], rooms: &[MapRoom]) {
    match screen.mode {
        AlertsMode::Browse => render_browse(f, area, screen, alerts),
        AlertsMode::Create => render_create(f, area, &screen.draft, rooms, &screen.status),
    }
}

// ---- Browse: today's alerts + stats ----------------------------------------

fn render_browse(f: &mut Frame, area: Rect, screen: &AlertsScreen, alerts: &[Alert]) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(56), Constraint::Percentage(44)])
        .split(area);

    render_today(f, cols[0], screen, alerts);

    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(cols[1]);
    render_stats(f, right[0], alerts);
    render_locations(f, right[1], alerts);
}

fn render_today(f: &mut Frame, area: Rect, screen: &AlertsScreen, alerts: &[Alert]) {
    let block = panel(" TODAY'S ALERTS ", true);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let today = domain::today_indices(alerts);
    if today.is_empty() {
        let note = Line::from(Span::styled(
            "No alerts today.  Press [N] to raise one.",
            Style::default().fg(PHOSPHOR_FAINT),
        ));
        let mid = Rect { y: inner.y + inner.height / 2, height: 1, ..inner };
        f.render_widget(Paragraph::new(note).alignment(Alignment::Center), mid);
        return;
    }

    let lines: Vec<Line> = today
        .iter()
        .enumerate()
        .map(|(row, &i)| alert_line(alerts, i, row == screen.selected))
        .collect();

    // Keep the selected row in view with a simple top-anchored scroll.
    let h = inner.height as usize;
    let scroll = screen.selected.saturating_sub(h.saturating_sub(1)) as u16;
    f.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
}

fn alert_line(alerts: &[Alert], i: usize, selected: bool) -> Line<'static> {
    let a = &alerts[i];
    let time = chrono::DateTime::parse_from_rfc3339(&a.created_at)
        .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_else(|_| "--:--".into());
    let loc = a.room_name.clone().unwrap_or_else(|| "ship".into());
    let mark = if a.delivered() { "\u{2713}" } else { "\u{00b7}" };
    let cursor = if selected { "\u{25B8} " } else { "  " };
    let base = if selected {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    Line::from(vec![
        Span::styled(cursor, Style::default().fg(AMBER)),
        Span::styled(format!("{:<5}", a.level.label()), base.fg(level_color(a.level))),
        Span::styled(format!(" {time} "), base.fg(PHOSPHOR_DIM)),
        Span::styled(format!("{mark} "), base.fg(if a.delivered() { PHOSPHOR } else { PHOSPHOR_FAINT })),
        Span::styled(format!("{loc}: "), base.fg(PHOSPHOR_DIM)),
        Span::styled(a.message.clone(), base.fg(PHOSPHOR)),
    ])
}

fn render_stats(f: &mut Frame, area: Rect, alerts: &[Alert]) {
    let block = panel(" STATS ", false);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let today = domain::today_indices(alerts).len();
    let pending = domain::pending_today(alerts);
    let by_level = domain::counts_by_level(alerts);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("TODAY    ", Style::default().fg(PHOSPHOR_DIM)),
            Span::styled(today.to_string(), Style::default().fg(PHOSPHOR).add_modifier(Modifier::BOLD)),
            Span::styled("    UNDELIVERED ", Style::default().fg(PHOSPHOR_DIM)),
            Span::styled(pending.to_string(), Style::default().fg(AMBER).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
    ];
    let max = by_level.iter().copied().max().unwrap_or(0).max(1);
    for (k, level) in AlertLevel::ALL.iter().enumerate() {
        let n = by_level[k];
        let filled = (n * GAUGE / max).min(GAUGE);
        let bar: String = "\u{2588}".repeat(filled);
        let rest: String = "\u{2500}".repeat(GAUGE - filled);
        lines.push(Line::from(vec![
            Span::styled(format!("{:<6}", level.label()), Style::default().fg(level_color(*level))),
            Span::styled(bar, Style::default().fg(level_color(*level))),
            Span::styled(rest, Style::default().fg(PHOSPHOR_FAINT)),
            Span::styled(format!(" {n}"), Style::default().fg(PHOSPHOR)),
        ]));
    }
    f.render_widget(Paragraph::new(lines), inner);
}

fn render_locations(f: &mut Frame, area: Rect, alerts: &[Alert]) {
    let block = panel(" BY LOCATION ", false);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rooms = domain::counts_by_room(alerts);
    if rooms.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No room-tagged alerts today.",
                Style::default().fg(PHOSPHOR_FAINT),
            ))),
            inner,
        );
        return;
    }
    let max = rooms.iter().map(|(_, n)| *n).max().unwrap_or(1).max(1);
    let lines: Vec<Line> = rooms
        .iter()
        .take(inner.height as usize)
        .map(|(name, n)| {
            let filled = (n * GAUGE / max).min(GAUGE);
            let bar: String = "\u{2588}".repeat(filled);
            let rest: String = "\u{2500}".repeat(GAUGE - filled);
            let label: String = name.chars().take(10).collect();
            Line::from(vec![
                Span::styled(format!("{label:<11}"), Style::default().fg(PHOSPHOR)),
                Span::styled(bar, Style::default().fg(AMBER)),
                Span::styled(rest, Style::default().fg(PHOSPHOR_FAINT)),
                Span::styled(format!(" {n}"), Style::default().fg(PHOSPHOR)),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

// ---- Create: raise an alert by hand ----------------------------------------

fn render_create(f: &mut Frame, area: Rect, draft: &Draft, rooms: &[MapRoom], status: &str) {
    let block = panel(" RAISE ALERT ", true);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // level
            Constraint::Length(3), // room
            Constraint::Length(3), // source
            Constraint::Length(3), // message
            Constraint::Length(3), // submit
            Constraint::Min(0),    // status
        ])
        .horizontal_margin(2)
        .split(inner);

    selector(f, rows[0], " LEVEL ", draft.level.label(), level_color(draft.level), draft.field == DraftField::Level);
    let room = match draft.room_idx {
        Some(i) => rooms.get(i).map(|r| r.name.clone()).unwrap_or_else(|| "—".into()),
        None => "— none (system) —".into(),
    };
    selector(f, rows[1], " ROOM ", &room, PHOSPHOR, draft.field == DraftField::Room);
    input_field(f, rows[2], " SOURCE ", &draft.source, draft.field == DraftField::Source);
    input_field(f, rows[3], " MESSAGE ", &draft.message, draft.field == DraftField::Message);
    button(f, rows[4], "RAISE ALERT", draft.field == DraftField::Submit);

    let status = if status.is_empty() {
        "Tab between fields · Left/Right cycles LEVEL & ROOM · Esc cancels".to_string()
    } else {
        status.to_string()
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(status, Style::default().fg(AMBER))))
            .wrap(Wrap { trim: true }),
        rows[5],
    );
}

/// A bracketed value cycled with Left/Right (LEVEL, ROOM).
fn selector(f: &mut Frame, area: Rect, title: &'static str, value: &str, value_color: Color, focused: bool) {
    let border = if focused { AMBER } else { PHOSPHOR_DIM };
    let spans = vec![
        Span::styled("\u{2039} ", Style::default().fg(PHOSPHOR_FAINT)),
        Span::styled(value.to_string(), Style::default().fg(value_color).add_modifier(Modifier::BOLD)),
        Span::styled(" \u{203A}", Style::default().fg(PHOSPHOR_FAINT)),
    ];
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .title(Span::styled(title, Style::default().fg(border)));
    f.render_widget(Paragraph::new(Line::from(spans)).block(block), area);
}

fn input_field(f: &mut Frame, area: Rect, title: &'static str, value: &str, focused: bool) {
    let border = if focused { AMBER } else { PHOSPHOR_DIM };
    let mut spans = vec![Span::styled(value.to_string(), Style::default().fg(PHOSPHOR))];
    if focused {
        spans.push(Span::styled("\u{2588}", Style::default().fg(PHOSPHOR)));
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .title(Span::styled(title, Style::default().fg(border)));
    f.render_widget(Paragraph::new(Line::from(spans)).block(block), area);
}

fn button(f: &mut Frame, area: Rect, label: &str, focused: bool) {
    let (block_style, text_style) = if focused {
        (
            Style::default().bg(AMBER).fg(Color::Black),
            Style::default().fg(Color::Black).add_modifier(Modifier::BOLD),
        )
    } else {
        (Style::default(), Style::default().fg(PHOSPHOR))
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(if focused { BorderType::Double } else { BorderType::Plain })
        .border_style(Style::default().fg(if focused { AMBER } else { PHOSPHOR_DIM }))
        .style(block_style);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(label.to_string(), text_style)))
            .alignment(Alignment::Center)
            .block(block),
        area,
    );
}
