//! Rendering for the boot screen: the scrolling self-test text and the
//! self-drawing MU/TH/UR circuit-board graphic.
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        canvas::{Canvas, Line as CanvasLine, Points},
        Block, BorderType, Borders, Paragraph, Wrap,
    },
    Frame,
};

use super::domain::{BootSequence, BOOT_SCRIPT};
use crate::ui::theme::{AMBER, PHOSPHOR, PHOSPHOR_DIM, PHOSPHOR_FAINT};

/// The Nostromo boot screen: self-test text scrolling down the left edge, the
/// MU/TH/UR circuit-board "diamond" drawing itself on the right. `sweep` drives
/// the blinking cursor; it's shared with the live interface's radar sweep.
pub fn render(f: &mut Frame, boot: &BootSequence, sweep: f64) {
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(PHOSPHOR_DIM))
        .title(Span::styled(" MU/TH/UR 6000 ", Style::default().fg(PHOSPHOR)));
    let inner = outer.inner(f.area());
    f.render_widget(outer, f.area());

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(inner);

    // ---- left: scrolling self-test text -----------------------------------
    let mut lines: Vec<Line> = vec![Line::from("")];
    for (i, raw) in BOOT_SCRIPT.iter().take(boot.revealed).enumerate() {
        let is_ready = i + 1 == BOOT_SCRIPT.len();
        let style = if is_ready {
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(PHOSPHOR)
        };
        lines.push(Line::from(Span::styled(format!(" {raw}"), style)));
    }
    // Blink a cursor for the whole boot screen — including the hold on READY —
    // so the long sequence never looks frozen.
    if (sweep * 4.0) as i64 % 2 == 0 {
        lines.push(Line::from(Span::styled(" \u{2588}", Style::default().fg(PHOSPHOR))));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), cols[0]);

    // ---- right: the circuit-board graphic ----------------------------------
    let progress = boot.revealed as f64 / BOOT_SCRIPT.len() as f64;
    render_mother_graphic(f, cols[1], progress);
}

/// Draws the symmetric PCB-diamond seen on the Nostromo mainframe. Traces are
/// authored once in the upper-right quadrant and mirrored across both axes for
/// the 4-fold symmetry. `progress` (0..1) reveals more of it as boot advances.
fn render_mother_graphic(f: &mut Frame, area: Rect, progress: f64) {
    // Hexagonal silhouette (wide, pointed left/right), drawn as a closed loop.
    const HEX: [(f64, f64); 6] = [
        (-0.92, 0.0),
        (-0.46, 0.62),
        (0.46, 0.62),
        (0.92, 0.0),
        (0.46, -0.62),
        (-0.46, -0.62),
    ];

    // PCB traces in ONE quadrant (Manhattan routing). Mirrored to all four.
    let q_traces: &[&[(f64, f64)]] = &[
        &[(0.05, 0.05), (0.32, 0.05), (0.32, 0.24), (0.58, 0.24)],
        &[(0.07, 0.0), (0.07, 0.34), (0.22, 0.34), (0.22, 0.5)],
        &[(0.0, 0.12), (0.46, 0.12), (0.46, 0.02)],
        &[(0.14, 0.14), (0.14, 0.28), (0.4, 0.28), (0.4, 0.42)],
        &[(0.26, 0.0), (0.26, 0.16), (0.66, 0.16)],
        &[(0.18, 0.06), (0.5, 0.06), (0.5, 0.32), (0.62, 0.32)],
    ];
    // Bright "solder node" points (also mirrored).
    let q_nodes: &[(f64, f64)] = &[(0.32, 0.24), (0.46, 0.12), (0.5, 0.32), (0.22, 0.5)];

    // How many traces are visible right now.
    let visible = ((q_traces.len() as f64) * (0.15 + progress * 0.85)).ceil() as usize;
    let visible = visible.min(q_traces.len());

    // Own the data inside the paint closure.
    let traces: Vec<Vec<(f64, f64)>> =
        q_traces.iter().take(visible).map(|t| t.to_vec()).collect();
    let nodes: Vec<(f64, f64)> = q_nodes.to_vec();
    let show_nodes = progress > 0.5;

    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([-1.0, 1.0])
        .y_bounds([-1.0, 1.0])
        .paint(move |ctx| {
            // outer silhouette
            for i in 0..HEX.len() {
                let (x1, y1) = HEX[i];
                let (x2, y2) = HEX[(i + 1) % HEX.len()];
                ctx.draw(&CanvasLine { x1, y1, x2, y2, color: PHOSPHOR_DIM });
            }
            ctx.layer();

            // traces, mirrored into all four quadrants
            for t in &traces {
                for &(sx, sy) in &[(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                    for w in t.windows(2) {
                        ctx.draw(&CanvasLine {
                            x1: w[0].0 * sx,
                            y1: w[0].1 * sy,
                            x2: w[1].0 * sx,
                            y2: w[1].1 * sy,
                            color: PHOSPHOR,
                        });
                    }
                }
            }
            ctx.layer();

            // center crosshair
            ctx.draw(&CanvasLine { x1: -0.9, y1: 0.0, x2: 0.9, y2: 0.0, color: PHOSPHOR_FAINT });
            ctx.draw(&CanvasLine { x1: 0.0, y1: -0.6, x2: 0.0, y2: 0.6, color: PHOSPHOR_FAINT });

            // bright nodes + glowing core
            if show_nodes {
                let mut pts = Vec::new();
                for &(nx, ny) in &nodes {
                    for &(sx, sy) in &[(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                        pts.push((nx * sx, ny * sy));
                    }
                }
                pts.push((0.0, 0.0));
                ctx.draw(&Points { coords: &pts, color: PHOSPHOR });
            }
        });
    f.render_widget(canvas, area);
}
