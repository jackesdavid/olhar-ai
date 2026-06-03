//! Rendering for the scan feature: the motion-tracker sonar (left) and the
//! device list (right). The sonar only animates while scanning or when contacts
//! are on file; otherwise it shows a static standby grid.
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        canvas::{Canvas, Circle, Line as CanvasLine, Points},
        Block, BorderType, Borders, Paragraph,
    },
    Frame,
};

use super::domain::{ScanState, TAU};
use crate::ui::chrome::panel;
use crate::ui::theme::{AMBER, PHOSPHOR, PHOSPHOR_DIM, PHOSPHOR_FAINT};

/// One device's fixed position on the sonar.
struct Contact {
    x: f64,
    y: f64,
    connected: bool, // valid credentials saved
    selected: bool,  // highlighted in the device list
}

/// A stable polar position for a device, derived from its IP so the dot doesn't
/// jump around between frames.
fn contact_pos(ip: &str) -> (f64, f64) {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in ip.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let angle = (h % 3600) as f64 / 3600.0 * TAU;
    let radius = 0.28 + ((h >> 20) % 1000) as f64 / 1000.0 * 0.58; // 0.28..0.86
    (angle.cos() * radius, angle.sin() * radius)
}

/// The sonar. Draws the sweep + one dot per discovered device when active; a dim
/// static grid otherwise. Connected cameras pulse; the selected one is ringed.
pub fn render_sonar(f: &mut Frame, area: Rect, scan: &ScanState) {
    let active = scan.is_active();
    let scanning = scan.scanning;
    let sweep = scan.tracker.sweep;

    let contacts: Vec<Contact> = scan
        .devices_snapshot()
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let (x, y) = contact_pos(&d.ip);
            Contact { x, y, connected: d.has_credentials(), selected: i == scan.selected }
        })
        .collect();

    let title = if scanning {
        " MOTION TRACKER  ·  SWEEPING "
    } else {
        " MOTION TRACKER "
    };

    let canvas = Canvas::default()
        .block(panel(title, false))
        .marker(Marker::Braille)
        .x_bounds([-1.0, 1.0])
        .y_bounds([-1.0, 1.0])
        .paint(move |ctx| {
            for r in [0.3_f64, 0.6, 0.9] {
                ctx.draw(&Circle { x: 0.0, y: 0.0, radius: r, color: PHOSPHOR_FAINT });
            }
            ctx.draw(&CanvasLine { x1: -0.9, y1: 0.0, x2: 0.9, y2: 0.0, color: PHOSPHOR_FAINT });
            ctx.draw(&CanvasLine { x1: 0.0, y1: -0.9, x2: 0.0, y2: 0.9, color: PHOSPHOR_FAINT });
            ctx.layer();

            if !active {
                return; // static grid only — no sweep, no contacts
            }

            // Rotating sweep ray with a dimmer trailing ray.
            let head = (sweep.cos() * 0.95, sweep.sin() * 0.95);
            ctx.draw(&CanvasLine { x1: 0.0, y1: 0.0, x2: head.0, y2: head.1, color: PHOSPHOR });
            let trail = sweep - 0.22;
            let t = (trail.cos() * 0.95, trail.sin() * 0.95);
            ctx.draw(&CanvasLine { x1: 0.0, y1: 0.0, x2: t.0, y2: t.1, color: PHOSPHOR_DIM });
            ctx.layer();

            // While a sweep is in progress, show only the rotating ray — the
            // contacts reappear once discovery + dedup finish.
            if scanning {
                return;
            }

            // 0..1 breathing value for the connected-device pulse.
            let pulse = (sweep * 3.0).sin() * 0.5 + 0.5;

            for c in &contacts {
                if c.selected {
                    // Amber ring marks the device selected in the list.
                    ctx.draw(&Circle { x: c.x, y: c.y, radius: 0.14, color: AMBER });
                }
                if c.connected {
                    // Bigger, pulsing "ping" for a connected camera.
                    let r = 0.06 + pulse * 0.06;
                    ctx.draw(&Circle { x: c.x, y: c.y, radius: r, color: PHOSPHOR });
                    ctx.draw(&Circle { x: c.x, y: c.y, radius: 0.035, color: PHOSPHOR });
                    ctx.draw(&Points { coords: &[(c.x, c.y)], color: PHOSPHOR });
                } else {
                    // Un-credentialed contact: a clearly visible small ring.
                    ctx.draw(&Circle { x: c.x, y: c.y, radius: 0.05, color: PHOSPHOR });
                    ctx.draw(&Points { coords: &[(c.x, c.y)], color: PHOSPHOR_DIM });
                }
            }
        });
    f.render_widget(canvas, area);

    if !active {
        // Overlay a STANDBY note in the middle of the static grid.
        let note = Line::from(Span::styled(
            "NO CONTACTS  ·  PRESS [D] TO DISCOVER",
            Style::default().fg(PHOSPHOR_DIM),
        ));
        let mid = Rect {
            x: area.x + 2,
            y: area.y + area.height / 2,
            width: area.width.saturating_sub(4),
            height: 1,
        };
        f.render_widget(Paragraph::new(note).alignment(Alignment::Center), mid);
    }
}

/// The right-hand list of scanned/saved devices, drawn as stacked boxes to match
/// the home menu — the selected device is a solid amber panel. While a sweep is
/// in progress the list is hidden behind a scanning notice.
pub fn render_device_list(f: &mut Frame, area: Rect, scan: &ScanState) {
    let outer = panel(" DEVICES ", true);
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    if scan.scanning {
        let lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "SCANNING\u{2026}",
                Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Probing for ONVIF transmitters\u{2026}",
                Style::default().fg(PHOSPHOR_DIM),
            )),
        ];
        f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
        return;
    }

    let devices = scan.devices_snapshot();
    if devices.is_empty() {
        let note = Line::from(Span::styled(
            "(no devices)  \u{00b7}  press [D] to discover",
            Style::default().fg(PHOSPHOR_FAINT),
        ));
        let mid = Rect { x: inner.x, y: inner.y + inner.height / 2, width: inner.width, height: 1 };
        f.render_widget(Paragraph::new(note).alignment(Alignment::Center), mid);
        return;
    }

    // top pad, then [box(3), blurb(1), gap(1)] per device — matching the menu.
    let mut constraints = vec![Constraint::Length(1)];
    for _ in &devices {
        constraints.push(Constraint::Length(3));
        constraints.push(Constraint::Length(1));
        constraints.push(Constraint::Length(1));
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    for (i, d) in devices.iter().enumerate() {
        let selected = i == scan.selected;
        let box_area = rows[1 + i * 3];
        let blurb_area = rows[2 + i * 3];

        let mark = if d.has_credentials() { "\u{2713}" } else { "\u{00b7}" };
        let label = format!("{:02}  {} {}", i + 1, mark, d.name);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(if selected {
                BorderType::Double
            } else {
                BorderType::Plain
            });
        let (block, label_style) = if selected {
            (
                block
                    .border_style(Style::default().fg(AMBER))
                    .style(Style::default().bg(AMBER).fg(Color::Black)),
                Style::default().fg(Color::Black).add_modifier(Modifier::BOLD),
            )
        } else {
            (
                block.border_style(Style::default().fg(PHOSPHOR_DIM)),
                Style::default().fg(PHOSPHOR),
            )
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(label, label_style)))
                .alignment(Alignment::Center)
                .block(block),
            box_area,
        );

        let mut blurb = d.ip.clone();
        if let Some(extra) = d.model.clone().or_else(|| d.manufacturer.clone()) {
            blurb.push_str("  \u{00b7}  ");
            blurb.push_str(&extra);
        }
        let blurb_style = if selected {
            Style::default().fg(AMBER)
        } else {
            Style::default().fg(PHOSPHOR_FAINT)
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(blurb, blurb_style))).alignment(Alignment::Center),
            blurb_area,
        );
    }
}
