//! Rendering for the Ship Configurations screen: a stack of labelled knobs —
//! horizontal bar-gauges for the volume levels and ON/OFF blocks for the
//! toggles. The focused row is a solid amber panel, matching the home menu and
//! device list. Values reflect [`Settings`] live.
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use super::domain::{Field, Settings, Transport, TYPING_MAX, TYPING_MIN};
use super::handler::ConfigScreen;
use crate::ui::chrome::panel;
use crate::ui::theme::{AMBER, PHOSPHOR, PHOSPHOR_DIM, PHOSPHOR_FAINT};

/// Number of cells in a level gauge.
const GAUGE: usize = 24;

pub fn render(f: &mut Frame, area: Rect, settings: &Settings, screen: &ConfigScreen) {
    let outer = panel(" SHIP CONFIGURATION ", true);
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    // top pad, then one Length(2) block (row + gap) per field.
    let mut constraints = vec![Constraint::Length(1)];
    for _ in Field::ALL {
        constraints.push(Constraint::Length(1));
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(0)); // footer note
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .horizontal_margin(2)
        .split(inner);

    for (i, field) in Field::ALL.iter().enumerate() {
        let row = rows[1 + i * 2];
        let focused = screen.focus == *field;
        render_row(f, row, *field, settings, focused);
    }

    let note = Line::from(Span::styled(
        "Levels feed the mixer live \u{00b7} settings persist to config.json",
        Style::default().fg(PHOSPHOR_FAINT),
    ));
    if let Some(footer) = rows.last() {
        f.render_widget(Paragraph::new(note).alignment(Alignment::Center), *footer);
    }
}

fn render_row(f: &mut Frame, area: Rect, field: Field, settings: &Settings, focused: bool) {
    // label (left) · control (right, fills)
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(18), Constraint::Min(0)])
        .split(area);

    let (label_style, accent) = if focused {
        (
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
            AMBER,
        )
    } else {
        (Style::default().fg(PHOSPHOR), PHOSPHOR)
    };

    let cursor = if focused { "\u{25B8} " } else { "  " };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(cursor, Style::default().fg(AMBER)),
            Span::styled(field.label(), label_style),
        ])),
        cols[0],
    );

    let control = match field {
        Field::Master => gauge(settings.master, accent),
        Field::Voice => gauge(settings.voice, accent),
        Field::Ui => gauge(settings.ui, accent),
        Field::Ambient => gauge(settings.ambient, accent),
        Field::VoiceEnabled => toggle(settings.voice_enabled, accent),
        Field::TypingSpeed => speed_gauge(settings.typing_speed, accent),
        Field::SttAutosend => toggle(settings.stt_autosend, accent),
        Field::RtspTransport => transport_selector(settings.transport, accent),
        Field::CrtSweep => toggle(settings.crt_sweep, accent),
        Field::CrtSweepIntensity => gauge(settings.crt_sweep_intensity, accent),
    };
    f.render_widget(Paragraph::new(control), cols[1]);
}

/// A 0..1 level as a filled bar plus a percentage readout.
fn gauge(value: f32, accent: Color) -> Line<'static> {
    let filled = ((value * GAUGE as f32).round() as usize).min(GAUGE);
    let bar: String = "\u{2588}".repeat(filled);
    let rest: String = "\u{2500}".repeat(GAUGE - filled);
    Line::from(vec![
        Span::styled(bar, Style::default().fg(accent)),
        Span::styled(rest, Style::default().fg(PHOSPHOR_FAINT)),
        Span::styled(format!("  {:>3}%", (value * 100.0).round() as u16), Style::default().fg(accent)),
    ])
}

/// Teletype speed shown as a small filled bar over its min..max range.
fn speed_gauge(value: u8, accent: Color) -> Line<'static> {
    let span = (TYPING_MAX - TYPING_MIN).max(1) as usize;
    let filled = (value.saturating_sub(TYPING_MIN) as usize * GAUGE / span).min(GAUGE);
    let bar: String = "\u{2588}".repeat(filled);
    let rest: String = "\u{2500}".repeat(GAUGE.saturating_sub(filled));
    Line::from(vec![
        Span::styled(bar, Style::default().fg(accent)),
        Span::styled(rest, Style::default().fg(PHOSPHOR_FAINT)),
        Span::styled(format!("  {} ch/tick", value), Style::default().fg(accent)),
    ])
}

/// A 3-way selector for the RTSP transport, e.g. `‹ TCP ›`, cycled with
/// Left/Right like the other adjustable rows.
fn transport_selector(t: Transport, accent: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled("\u{2039} ", Style::default().fg(PHOSPHOR_FAINT)),
        Span::styled(
            format!(" {} ", t.label()),
            Style::default().bg(accent).fg(Color::Black).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" \u{203A}", Style::default().fg(PHOSPHOR_FAINT)),
    ])
}

/// A boolean rendered as a highlighted ON / OFF block.
fn toggle(on: bool, accent: Color) -> Line<'static> {
    let (txt, style) = if on {
        (
            "  ON  ",
            Style::default().bg(accent).fg(Color::Black).add_modifier(Modifier::BOLD),
        )
    } else {
        ("  OFF ", Style::default().fg(PHOSPHOR_DIM))
    };
    Line::from(Span::styled(txt, style))
}
