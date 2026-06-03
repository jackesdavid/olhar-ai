//! Rendering for the MAP screen: the floor-plan canvas on the left and a context
//! panel (selected-room detail, or the camera checklist) on the right.
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        canvas::{Canvas, Line as CanvasLine, Rectangle as CanvasRect},
        Block, BorderType, Borders, Paragraph,
    },
    Frame,
};

use super::domain::{MapMode, MapState, GRID_H, GRID_W};
use crate::scan::ScanState;
use crate::ui::chrome::panel;
use crate::ui::theme::{AMBER, PHOSPHOR, PHOSPHOR_DIM, PHOSPHOR_FAINT};

pub fn render(f: &mut Frame, area: Rect, map: &MapState, scan: &ScanState) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(area);
    render_floor(f, cols[0], map);
    render_context(f, cols[1], map, scan);
}

/// The left floor-plan canvas: a faint grid, the room rectangles (y-flipped so a
/// y-down grid reads right-side up), labels, and per-room camera markers.
fn render_floor(f: &mut Frame, area: Rect, map: &MapState) {
    let gw = GRID_W as f64;
    let gh = GRID_H as f64;
    let rooms = map.rooms.clone();
    let selected = map.selected;

    let canvas = Canvas::default()
        .block(panel(" FLOOR PLAN ", false))
        .marker(Marker::Braille)
        .x_bounds([0.0, gw])
        .y_bounds([0.0, gh])
        .paint(move |ctx| {
            for gx in 0..=GRID_W {
                ctx.draw(&CanvasLine {
                    x1: gx as f64, y1: 0.0, x2: gx as f64, y2: gh, color: PHOSPHOR_FAINT,
                });
            }
            for gy in 0..=GRID_H {
                ctx.draw(&CanvasLine {
                    x1: 0.0, y1: gy as f64, x2: gw, y2: gy as f64, color: PHOSPHOR_FAINT,
                });
            }
            ctx.layer();

            // Rooms. Rectangle is positioned from its BOTTOM-LEFT corner and the
            // canvas y grows UP, but the grid is y-DOWN top-left, so flip:
            // bottom-left y = GRID_H - (room.y + room.h).
            for (i, r) in rooms.iter().enumerate() {
                let color = if i == selected { AMBER } else { PHOSPHOR_DIM };
                let fy = gh - (r.y as f64 + r.h as f64);
                ctx.draw(&CanvasRect {
                    x: r.x as f64, y: fy, width: r.w as f64, height: r.h as f64, color,
                });
                if i == selected {
                    // Inset second outline to read as the lit selection.
                    ctx.draw(&CanvasRect {
                        x: r.x as f64 + 0.15,
                        y: fy + 0.15,
                        width: (r.w as f64 - 0.3).max(0.1),
                        height: (r.h as f64 - 0.3).max(0.1),
                        color: AMBER,
                    });
                }
            }
            ctx.layer();

            for (i, r) in rooms.iter().enumerate() {
                let (cx, cy_grid) = r.center();
                let cy = gh - cy_grid;
                let name_color = if i == selected { AMBER } else { PHOSPHOR };
                ctx.print(
                    cx - r.name.chars().count() as f64 / 2.0,
                    cy,
                    Line::from(Span::styled(r.name.clone(), Style::default().fg(name_color))),
                );
                if !r.cameras.is_empty() {
                    ctx.print(
                        cx - 1.0,
                        cy - 1.0,
                        Line::from(Span::styled(
                            format!("\u{25A3}{}", r.cameras.len()),
                            Style::default().fg(PHOSPHOR),
                        )),
                    );
                }
            }
        });
    f.render_widget(canvas, area);

    if map.rooms.is_empty() {
        let note = Line::from(Span::styled(
            "NO ROOMS  \u{00b7}  PRESS [N] TO ADD",
            Style::default().fg(PHOSPHOR_DIM),
        ));
        let mid = Rect { x: area.x + 2, y: area.y + area.height / 2, width: area.width.saturating_sub(4), height: 1 };
        f.render_widget(Paragraph::new(note).alignment(Alignment::Center), mid);
    }
}

/// The right panel: selected-room detail, or the camera checklist in Cameras mode.
fn render_context(f: &mut Frame, area: Rect, map: &MapState, scan: &ScanState) {
    let outer = panel(" ROOM ", true);
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    // Bottom row reserved for the status line.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);

    if map.mode == MapMode::Cameras {
        render_camera_picker(f, rows[0], map, scan);
    } else {
        render_detail(f, rows[0], map, scan);
    }

    if !map.status.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(map.status.clone(), Style::default().fg(AMBER))))
                .alignment(Alignment::Center),
            rows[1],
        );
    }
}

fn render_detail(f: &mut Frame, area: Rect, map: &MapState, scan: &ScanState) {
    let Some(room) = map.selected_room() else {
        let note = Line::from(Span::styled(
            "No room selected.  [N] new room",
            Style::default().fg(PHOSPHOR_FAINT),
        ));
        f.render_widget(Paragraph::new(note).alignment(Alignment::Center), area);
        return;
    };

    let mut lines: Vec<Line> = Vec::new();

    // Name (block cursor while renaming).
    let renaming = map.mode == MapMode::Rename;
    let name = if renaming { format!("{}\u{2588}", room.name) } else { room.name.clone() };
    lines.push(Line::from(vec![
        Span::styled("NAME  ", Style::default().fg(PHOSPHOR_DIM)),
        Span::styled(name, Style::default().fg(AMBER).add_modifier(Modifier::BOLD)),
    ]));
    lines.push(Line::from(vec![
        Span::styled("POS   ", Style::default().fg(PHOSPHOR_DIM)),
        Span::styled(format!("{}, {}", room.x, room.y), Style::default().fg(PHOSPHOR)),
        Span::styled("    SIZE  ", Style::default().fg(PHOSPHOR_DIM)),
        Span::styled(format!("{} \u{00d7} {}", room.w, room.h), Style::default().fg(PHOSPHOR)),
    ]));
    if map.mode != MapMode::Browse {
        let tag = match map.mode {
            MapMode::Move => "MODE: MOVE",
            MapMode::Resize => "MODE: RESIZE",
            MapMode::Rename => "MODE: RENAME",
            _ => "",
        };
        lines.push(Line::from(Span::styled(tag, Style::default().fg(AMBER))));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("CAMERAS", Style::default().fg(PHOSPHOR_DIM))));

    // Resolve assigned cameras to live device names; skip stale ips.
    let mut shown = 0;
    for ip in &room.cameras {
        if let Some(d) = scan.device_by_ip(ip) {
            shown += 1;
            lines.push(Line::from(vec![
                Span::styled(" \u{2713} ", Style::default().fg(PHOSPHOR)),
                Span::styled(d.name.clone(), Style::default().fg(PHOSPHOR)),
                Span::styled(format!("  {}", d.ip), Style::default().fg(PHOSPHOR_FAINT)),
            ]));
        }
    }
    if shown == 0 {
        lines.push(Line::from(Span::styled(" (none)  \u{00b7}  [C] assign", Style::default().fg(PHOSPHOR_FAINT))));
    }

    f.render_widget(Paragraph::new(lines), area);
}

/// The device checklist used in Cameras mode: a tick when the device is assigned
/// to the selected room, the cursor row drawn as a solid amber panel.
fn render_camera_picker(f: &mut Frame, area: Rect, map: &MapState, scan: &ScanState) {
    let devices = scan.devices_snapshot();
    let assigned: &[String] = map.selected_room().map_or(&[], |r| r.cameras.as_slice());

    if devices.is_empty() {
        let note = Line::from(Span::styled(
            "(no devices)  \u{00b7}  scan first",
            Style::default().fg(PHOSPHOR_FAINT),
        ));
        let mid = Rect { x: area.x, y: area.y + area.height / 2, width: area.width, height: 1 };
        f.render_widget(Paragraph::new(note).alignment(Alignment::Center), mid);
        return;
    }

    // top pad, then [box(3), gap(1)] per device.
    let mut constraints = vec![Constraint::Length(1)];
    for _ in &devices {
        constraints.push(Constraint::Length(3));
        constraints.push(Constraint::Length(1));
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    for (i, d) in devices.iter().enumerate() {
        let Some(&box_area) = rows.get(1 + i * 2) else { break };
        let selected = i == map.cam_cursor;
        let on = assigned.iter().any(|ip| ip == &d.ip);
        let mark = if on { "\u{2713}" } else { "\u{00b7}" };
        let label = format!("{} {}", mark, d.name);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(if selected { BorderType::Double } else { BorderType::Plain });
        let (block, label_style) = if selected {
            (
                block.border_style(Style::default().fg(AMBER)).style(Style::default().bg(AMBER).fg(Color::Black)),
                Style::default().fg(Color::Black).add_modifier(Modifier::BOLD),
            )
        } else {
            (block.border_style(Style::default().fg(PHOSPHOR_DIM)), Style::default().fg(PHOSPHOR))
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(label, label_style)))
                .alignment(Alignment::Center)
                .block(block),
            box_area,
        );
    }
}
