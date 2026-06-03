//! The BEACONS screen: the trusted notification devices, drawn as a grid of
//! rotating beacon wireframes (each paired phone is a "comm beacon"). Beacons are
//! enrolled by pairing from Telegram (`/pair <code>`); from here the operator can
//! open one, rename it, test it, mute it, or remove it. Also hosts the
//! beacon-detail view (a rotating beacon on one side, a rename form on the other).
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{canvas::Canvas, Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

use super::domain::{NotificationConfig, NotificationTarget};
use super::handler::{BeaconDetail, BeaconField, BeaconsScreen, BEACON_COLS};
use crate::ui::chrome::panel;
use crate::ui::theme::{AMBER, PHOSPHOR, PHOSPHOR_DIM, PHOSPHOR_FAINT};
use crate::ui::wireframe;

/// Render the beacon grid (a snapshot, cloned by the caller) plus a status line.
/// `phase` (the global sweep) spins each beacon wireframe.
pub fn render(
    f: &mut Frame,
    area: Rect,
    targets: &[NotificationTarget],
    screen: &BeaconsScreen,
    phase: f64,
) {
    let outer = panel(" BEACONS ", true);
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    if targets.is_empty() {
        render_empty(f, inner);
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);
    render_grid(f, rows[0], targets, screen, phase);

    if !screen.status.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                screen.status.clone(),
                Style::default().fg(AMBER),
            )))
            .alignment(Alignment::Center),
            rows[1],
        );
    }
}

fn render_grid(
    f: &mut Frame,
    area: Rect,
    targets: &[NotificationTarget],
    screen: &BeaconsScreen,
    phase: f64,
) {
    let cols = BEACON_COLS;
    let grid_rows = targets.len().div_ceil(cols);
    let row_rects = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![Constraint::Ratio(1, grid_rows as u32); grid_rows])
        .split(area);

    for (i, t) in targets.iter().enumerate() {
        let r = i / cols;
        let c = i % cols;
        let cell = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Ratio(1, cols as u32); cols])
            .split(row_rects[r])[c];
        render_cell(f, cell, i, t, i == screen.selected, phase);
    }
}

/// One grid cell: a bordered box with a small spinning beacon, the label, and the
/// online/muted state. The selected cell gets the amber double border.
fn render_cell(f: &mut Frame, area: Rect, idx: usize, t: &NotificationTarget, selected: bool, phase: f64) {
    let border = if selected { AMBER } else { PHOSPHOR_DIM };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(if selected { BorderType::Double } else { BorderType::Plain })
        .border_style(Style::default().fg(border));
    let cell = block.inner(area);
    f.render_widget(block, area);

    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1), Constraint::Length(1)])
        .split(cell);

    // Offset each beacon's phase so the grid doesn't pulse in lockstep.
    let p = phase + idx as f64 * 0.7;
    let beacon = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([-1.0, 1.0])
        .y_bounds([-1.0, 1.0])
        .paint(move |ctx| wireframe::draw_beacon(ctx, p));
    f.render_widget(beacon, parts[0]);

    let label_style = if selected {
        Style::default().fg(AMBER).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(PHOSPHOR)
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(format!("{:02} {}", idx + 1, t.label), label_style)))
            .alignment(Alignment::Center),
        parts[1],
    );

    let mark = if t.enabled { "\u{25CF}" } else { "\u{25CB}" };
    let state = if t.enabled { "ONLINE" } else { "MUTED" };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("{mark} {state}"),
            Style::default().fg(if selected { AMBER } else { PHOSPHOR_FAINT }),
        )))
        .alignment(Alignment::Center),
        parts[2],
    );
}

/// Empty state: explain how to pair a phone, showing the live `/pair` code when
/// one is configured.
fn render_empty(f: &mut Frame, area: Rect) {
    let pair_line = match NotificationConfig::from_env() {
        Some(cfg) if cfg.can_pair() => {
            format!("From Telegram, send the bot:  /pair {}", cfg.pairing_code)
        }
        Some(_) => "Set TELEGRAM_PAIRING_CODE, then /pair it from Telegram.".to_string(),
        None => "Set TELEGRAM_BOT_TOKEN and TELEGRAM_PAIRING_CODE to enable beacons.".to_string(),
    };
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "NO BEACONS PAIRED",
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(pair_line, Style::default().fg(PHOSPHOR_DIM))),
        Line::from(Span::styled(
            "A phone becomes a beacon once it sends the code.",
            Style::default().fg(PHOSPHOR_FAINT),
        )),
    ];
    let mid = Rect {
        x: area.x,
        y: area.y + area.height.saturating_sub(5) / 2,
        width: area.width,
        height: 5.min(area.height),
    };
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), mid);
}

// ---- beacon detail ----------------------------------------------------------

/// The beacon-detail screen: a rename form on the left, a rotating beacon on the
/// right. `phase` spins the beacon.
pub fn render_detail(
    f: &mut Frame,
    area: Rect,
    detail: &BeaconDetail,
    target: &NotificationTarget,
    phase: f64,
) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // beacon info
            Constraint::Length(3), // name
            Constraint::Length(3), // action buttons
            Constraint::Min(0),    // status
        ])
        .split(cols[0]);

    // ---- beacon info (read-only) ------------------------------------------
    let info = vec![
        Line::from(vec![
            Span::styled("PROVIDER ", Style::default().fg(PHOSPHOR_DIM)),
            Span::styled("TELEGRAM", Style::default().fg(PHOSPHOR)),
        ]),
        Line::from(vec![
            Span::styled("CHAT ID  ", Style::default().fg(PHOSPHOR_DIM)),
            Span::styled(target.chat_id.to_string(), Style::default().fg(PHOSPHOR)),
        ]),
    ];
    f.render_widget(
        Paragraph::new(info).block(panel(" BEACON ", true)).wrap(Wrap { trim: true }),
        rows[0],
    );

    // ---- editable name ----------------------------------------------------
    input_field(f, rows[1], " NAME ", &detail.name, detail.field == BeaconField::Name);

    // ---- action buttons ---------------------------------------------------
    let mute_label = if target.enabled { "MUTE" } else { "UNMUTE" };
    let btns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(26),
            Constraint::Percentage(24),
            Constraint::Percentage(16),
        ])
        .split(rows[2]);
    button(f, btns[0], "RENAME", detail.field == BeaconField::Rename);
    button(f, btns[1], "TEST", detail.field == BeaconField::Test);
    button(f, btns[2], mute_label, detail.field == BeaconField::Mute);
    button(f, btns[3], "BACK", detail.field == BeaconField::Back);

    // ---- status -----------------------------------------------------------
    let status = if detail.status.is_empty() {
        "Tab between fields. Edit NAME, then activate RENAME to save.".to_string()
    } else {
        detail.status.clone()
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(status, Style::default().fg(AMBER))))
            .block(panel(" STATUS ", false))
            .wrap(Wrap { trim: true }),
        rows[3],
    );

    // ---- rotating beacon on the right -------------------------------------
    let block = panel(" SIGNAL ", false);
    let beacon_area = block.inner(cols[1]);
    f.render_widget(block, cols[1]);
    let beacon = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([-1.0, 1.0])
        .y_bounds([-1.0, 1.0])
        .paint(move |ctx| wireframe::draw_beacon(ctx, phase));
    f.render_widget(beacon, beacon_area);
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
