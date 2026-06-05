//! The shared live-feed renderer, used both in the device-detail right column
//! and in the chat modal — only the host `Rect` differs.
use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::domain::{LiveStream, StreamState, MAX_RECONNECT_ATTEMPTS};
use crate::ui::theme::{AMBER, PHOSPHOR_DIM, PHOSPHOR_FAINT};

/// Ticks without a new frame before we call the feed stalled (~2s at 50ms).
const STALL_LIMIT: u32 = 40;

/// Render the current state of a live stream into `area`.
pub fn render_feed(f: &mut Frame, area: Rect, stream: &LiveStream) {
    match (&stream.state, &stream.latest) {
        // Terminal failure — fatal error or reconnects exhausted.
        (StreamState::Failed(reason), _) => {
            note(f, area, &format!("STREAM FAILED \u{2014} {reason}"));
        }
        // Reconnecting — keep the last frame visible behind the notice if we
        // have one, so the feed doesn't blink to black on a brief drop.
        (StreamState::Reconnecting { attempt }, latest) => {
            if let Some(img) = latest {
                img.render(f, area);
            }
            note(
                f,
                area,
                &format!("RECONNECTING\u{2026} {attempt}/{MAX_RECONNECT_ATTEMPTS}"),
            );
        }
        (_, Some(img)) if stream.stalled_ticks > STALL_LIMIT => {
            img.render(f, area);
            note(f, area, "NO SIGNAL");
        }
        (_, Some(img)) => {
            img.render(f, area);
            live_badge(f, area);
        }
        (_, None) => note(f, area, "CONNECTING\u{2026}"),
    }
}

/// A centered single-line status note.
fn note(f: &mut Frame, area: Rect, text: &str) {
    if area.height == 0 {
        return;
    }
    let line = Line::from(Span::styled(text.to_string(), Style::default().fg(PHOSPHOR_FAINT)));
    let mid = Rect {
        x: area.x,
        y: area.y + area.height / 2,
        width: area.width,
        height: 1,
    };
    f.render_widget(Paragraph::new(line).alignment(Alignment::Center), mid);
}

/// A small "● LIVE" badge in the top-left corner.
fn live_badge(f: &mut Frame, area: Rect) {
    let badge = Line::from(vec![
        Span::styled(" \u{25CF} ", Style::default().fg(AMBER)),
        Span::styled("LIVE", Style::default().fg(PHOSPHOR_DIM).add_modifier(Modifier::BOLD)),
    ]);
    let r = Rect { x: area.x, y: area.y, width: area.width.min(10), height: 1 };
    f.render_widget(Paragraph::new(badge), r);
}
