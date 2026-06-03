//! Rendering for the splash card: a rotating wireframe of the Nostromo as the
//! hero logo, with Weyland-Yutani branding above and a compact NOSTROMO wordmark
//! plus ship/system identifiers below.
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{canvas::Canvas, Block, BorderType, Borders, Paragraph},
    Frame,
};

use super::domain::SplashSequence;
use crate::ui::theme::{AMBER, PHOSPHOR, PHOSPHOR_DIM, PHOSPHOR_FAINT};
use crate::ui::wireframe;

/// Height (rows) reserved for the spinning logo — the centerpiece of the card.
const LOGO_H: u16 = 14;
/// Logo width cap (cols) so the wireframe stays proportioned instead of
/// stretching across a very wide window.
const LOGO_W: u16 = 64;

/// Draw the title card. `phase` (the global sweep) spins the logo; `_splash` is
/// accepted for symmetry with the boot screen and a future reveal animation.
pub fn render(f: &mut Frame, _splash: &SplashSequence, phase: f64) {
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(PHOSPHOR_DIM))
        .title(Span::styled(" NOSTROMO ", Style::default().fg(PHOSPHOR)));
    let inner = outer.inner(f.area());
    f.render_widget(outer, f.area());

    // corp, gap, logo, wordmark, tagline, gap, rule, footer×2
    let content_h = 1 + 1 + LOGO_H + 1 + 1 + 1 + 1 + 1 + 1;
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(content_h),
            Constraint::Min(0),
        ])
        .split(inner);

    let stack = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),      // WEYLAND-YUTANI
            Constraint::Length(1),      // gap
            Constraint::Length(LOGO_H), // rotating logo
            Constraint::Length(1),      // NOSTROMO wordmark
            Constraint::Length(1),      // tagline
            Constraint::Length(1),      // gap
            Constraint::Length(1),      // rule
            Constraint::Length(1),      // footer line 1
            Constraint::Length(1),      // footer line 2
        ])
        .split(rows[1]);

    let center = |s: &str, style: Style| {
        Paragraph::new(Line::from(Span::styled(s.to_string(), style))).alignment(Alignment::Center)
    };

    f.render_widget(
        center("WEYLAND-YUTANI CORPORATION", Style::default().fg(PHOSPHOR_DIM)),
        stack[0],
    );

    // Rotating wireframe ship — the Nostromo herself — centered in a capped-width
    // box so she keeps her proportions on wide screens.
    let logo_w = LOGO_W.min(stack[2].width);
    let logo_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(logo_w),
            Constraint::Min(0),
        ])
        .split(stack[2])[1];
    let logo = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds([-1.0, 1.0])
        .y_bounds([-1.0, 1.0])
        .paint(move |ctx| wireframe::draw_ship(ctx, phase));
    f.render_widget(logo, logo_area);

    f.render_widget(
        center(
            "N O S T R O M O",
            Style::default().fg(PHOSPHOR).add_modifier(Modifier::BOLD),
        ),
        stack[3],
    );
    f.render_widget(
        center(
            "S U R V E I L L A N C E   S Y S T E M",
            Style::default().fg(PHOSPHOR_DIM),
        ),
        stack[4],
    );

    let rule_w = 44u16.min(inner.width.saturating_sub(2));
    f.render_widget(
        center(
            &"─".repeat(rule_w as usize),
            Style::default().fg(PHOSPHOR_FAINT),
        ),
        stack[6],
    );

    f.render_widget(
        center(
            "USCSS NOSTROMO 180924609   ·   MU/TH/UR 6000",
            Style::default().fg(PHOSPHOR_DIM),
        ),
        stack[7],
    );
    f.render_widget(
        center(
            "BISHOP/AP   INTERFACE 2037",
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
        ),
        stack[8],
    );
}
