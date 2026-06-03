//! Reusable window chrome: bordered panels and the top status bar.
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use super::theme::{AMBER, PHOSPHOR, PHOSPHOR_DIM, PHOSPHOR_FAINT};

/// A single centered, dim line of key hints — used as a screen footer.
pub fn render_hint(f: &mut Frame, area: Rect, text: &str) {
    let line = Line::from(Span::styled(
        text.to_string(),
        Style::default().fg(PHOSPHOR_DIM),
    ));
    f.render_widget(Paragraph::new(line).alignment(Alignment::Center), area);
}

/// A titled bordered panel in the house style. `double` picks the heavier
/// double-line border (used for framing the boot screen and input box).
pub fn panel(title: &'static str, double: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(if double {
            BorderType::Double
        } else {
            BorderType::Rounded
        })
        .border_style(Style::default().fg(PHOSPHOR_DIM))
        .title(Span::styled(title, Style::default().fg(PHOSPHOR)))
}

/// The persistent Weyland-Yutani / MU/TH/UR status bar across the top of every
/// screen. A processing indicator sits on the left, the title in the centre, and
/// the live audio meter on the right. `busy` animates the thinking spinner;
/// `listening` overrides it with a "LISTENING" state while the voice link is
/// capturing; `audio_active` lights the meter; `phase` (an ever-advancing value,
/// e.g. the radar sweep) animates them.
pub fn render_topbar(
    f: &mut Frame,
    area: Rect,
    busy: bool,
    listening: bool,
    audio_active: bool,
    phase: f64,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(PHOSPHOR_DIM));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // status (left) · title (centre, fills) · audio meter (right)
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(18),
            Constraint::Min(0),
            Constraint::Length(20),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(status_indicator(busy, listening, phase)).alignment(Alignment::Left),
        cols[0],
    );

    let title = Line::from(vec![
        Span::styled("WEYLAND-YUTANI CORP   ", Style::default().fg(PHOSPHOR_DIM)),
        Span::styled(
            "< MU/TH/UR 6000 >",
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
        ),
        Span::styled("   BISHOP/AP", Style::default().fg(PHOSPHOR_DIM)),
    ]);
    f.render_widget(Paragraph::new(title).alignment(Alignment::Center), cols[1]);

    f.render_widget(
        Paragraph::new(audio_meter(audio_active, phase)).alignment(Alignment::Right),
        cols[2],
    );
}

/// The left-hand processing indicator: a pulsing "LISTENING" while the voice
/// link captures (highest priority), an animated braille spinner with
/// "PROCESSING" while Bishop is busy, or an idle "[F9] TALK" prompt advertising
/// the voice link so the operator knows they can press F9 to speak to Bishop.
fn status_indicator(busy: bool, listening: bool, phase: f64) -> Line<'static> {
    const SPINNER: [char; 10] =
        ['\u{280B}', '\u{2819}', '\u{2839}', '\u{2838}', '\u{283C}', '\u{2834}', '\u{2826}', '\u{2827}', '\u{2807}', '\u{280F}'];

    if listening {
        // Blink the ● so it reads as actively recording.
        let on = (phase * 4.0) as usize % 2 == 0;
        let dot = if on { " \u{25CF} " } else { "   " };
        Line::from(vec![
            Span::styled(dot, Style::default().fg(AMBER).add_modifier(Modifier::BOLD)),
            Span::styled("LISTENING", Style::default().fg(AMBER).add_modifier(Modifier::BOLD)),
        ])
    } else if busy {
        let frame = ((phase * 6.0) as usize) % SPINNER.len();
        Line::from(vec![
            Span::styled(
                format!(" {} ", SPINNER[frame]),
                Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
            ),
            Span::styled("PROCESSING", Style::default().fg(AMBER)),
        ])
    } else {
        // Idle: advertise the F9 voice link. A slow pulse on the key draws the
        // eye without nagging; "TALK" reads as the action it triggers.
        let bright = (phase * 1.5) as usize % 2 == 0;
        let key_style = Style::default()
            .fg(if bright { AMBER } else { PHOSPHOR_DIM })
            .add_modifier(Modifier::BOLD);
        Line::from(vec![
            Span::styled(" \u{25C6} ", Style::default().fg(PHOSPHOR_DIM)),
            Span::styled("[F9]", key_style),
            Span::styled(" TALK", Style::default().fg(PHOSPHOR_DIM)),
        ])
    }
}

/// Builds the right-hand audio meter: a row of bouncing bars while sound plays,
/// or a flat dim trace when idle.
fn audio_meter(active: bool, phase: f64) -> Line<'static> {
    const BARS: [char; 8] = ['\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}'];
    const N: usize = 8;

    if active {
        // Pseudo-levels from layered sines so the bars feel lively, not uniform.
        let mut s = String::with_capacity(N);
        for i in 0..N {
            let w = (phase * 3.1 + i as f64 * 0.8).sin() * 0.5
                + (phase * 7.3 + i as f64 * 1.7).sin() * 0.3
                + 0.2;
            let level = (((w * 0.5 + 0.5) * (BARS.len() - 1) as f64).round() as usize)
                .min(BARS.len() - 1);
            s.push(BARS[level]);
        }
        Line::from(vec![
            Span::styled("\u{266A} ", Style::default().fg(AMBER)),
            Span::styled(s, Style::default().fg(PHOSPHOR)),
            Span::styled(" ", Style::default()),
        ])
    } else {
        Line::from(vec![
            Span::styled("\u{266A} ", Style::default().fg(PHOSPHOR_FAINT)),
            Span::styled("\u{2500}".repeat(N), Style::default().fg(PHOSPHOR_FAINT)),
            Span::styled(" ", Style::default()),
        ])
    }
}
