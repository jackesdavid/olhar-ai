//! Top-level view: dispatch by screen and compose each screen's layout from the
//! shared chrome plus the relevant feature views.
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
    Frame,
};

use super::domain::{App, Screen};
use crate::about;
use crate::boot;
use crate::chat::view::{render_input, render_log};
use crate::device;
use crate::menu::view::{render_help, render_list, render_visual};
use crate::scan::view::{render_device_list, render_sonar};
use crate::ui::chrome::{render_hint, render_topbar};
use crate::ui::theme::{AMBER, PHOSPHOR, PHOSPHOR_DIM};

pub fn draw(f: &mut Frame, app: &App) {
    match app.screen {
        Screen::Boot => boot::view::render(f, &app.boot, app.scan.tracker.sweep),
        Screen::Menu => render_menu(f, app),
        Screen::Scan => render_scan(f, app),
        Screen::DeviceDetail => render_device_detail(f, app),
        Screen::Chat => render_chat(f, app),
        Screen::About => render_about(f, app),
        Screen::Config => render_config(f, app),
        Screen::Alerts => render_alerts(f, app),
        Screen::Map => render_map(f, app),
    }

    // The voice link overlays every screen so you always know it's listening.
    if app.listen.active {
        render_listening(f, app);
    }
}

/// Standard screen frame: persistent top bar, body in the middle, a hint footer
/// at the bottom. Returns the body `Rect` for the caller to fill.
fn framed(f: &mut Frame, app: &App, hint: &str) -> Rect {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(f.area());

    render_topbar(
        f,
        rows[0],
        app.activity.is_busy(),
        app.listen.active,
        app.sfx.is_active(),
        app.scan.tracker.sweep,
    );
    render_hint(f, rows[2], hint);
    rows[1]
}

fn render_menu(f: &mut Frame, app: &App) {
    let body = framed(
        f,
        app,
        "[ Up/Down ] move    [ Enter ] select    [ Esc ] quit",
    );
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(body);
    render_list(f, cols[0], &app.menu);

    // Right column: a themed 3D wireframe schematic on top, briefing text below.
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(cols[1]);
    render_visual(f, right[0], &app.menu, app.scan.tracker.sweep);
    render_help(f, right[1], &app.menu);
}

fn render_scan(f: &mut Frame, app: &App) {
    let body = framed(
        f,
        app,
        "[ D ] discover   [ Up/Down ] select   [ Enter ] open   [ X ] delete   [ Esc ] menu",
    );
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(body);
    render_sonar(f, cols[0], &app.scan);
    render_device_list(f, cols[1], &app.scan);
}

fn render_device_detail(f: &mut Frame, app: &App) {
    let body = framed(
        f,
        app,
        "[ Tab ] field    [ Enter ] activate    [ Esc ] back",
    );
    if let Some(detail) = &app.detail {
        if let Some(device) = app.scan.device_by_ip(&detail.ip) {
            device::view::render(f, body, detail, &device, app.live.as_ref());
        }
    }
}

fn render_chat(f: &mut Frame, app: &App) {
    // Chat owns its bottom row (the input box), so it builds its own layout
    // instead of using the hint footer.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(f.area());

    render_topbar(
        f,
        rows[0],
        app.activity.is_busy(),
        app.listen.active,
        app.sfx.is_active(),
        app.scan.tracker.sweep,
    );
    render_log(f, rows[1], &app.convo);
    render_input(f, rows[2], &app.convo);

    // A snapshot or live feed Bishop opened, shown as a modal over the chat.
    if let Some(img) = &app.snapshot_modal {
        render_modal(f, " CAMERA  ·  [Esc] close ", |f, inner| {
            img.render(f, inner)
        });
    } else if app.live_modal {
        if let Some(s) = &app.live {
            render_modal(f, " LIVE FEED  ·  [Esc] close ", |f, inner| {
                crate::stream::view::render_feed(f, inner, s)
            });
        }
    }
}

/// A centered, background-clearing modal with a double border; `body` fills it.
fn render_modal(f: &mut Frame, title: &str, body: impl FnOnce(&mut Frame, Rect)) {
    let area = centered_rect(74, 80, f.area());
    f.render_widget(Clear, area); // wipe the chat behind the modal
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(PHOSPHOR_DIM))
        .title(Span::styled(title.to_string(), Style::default().fg(PHOSPHOR)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    body(f, inner);
}

/// The voice-link overlay: a centered box with a live microphone waveform so
/// the operator can see the system is listening. Drawn over any screen.
fn render_listening(f: &mut Frame, app: &App) {
    let area = centered_rect(56, 34, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(AMBER))
        .title(Span::styled(
            " \u{25CF} LISTENING ",
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // waveform (vertically centered)
            Constraint::Length(1), // prompt
            Constraint::Length(1), // hint
        ])
        .split(inner);

    let level = app.listen.level();
    let phase = app.scan.tracker.sweep;
    f.render_widget(
        Paragraph::new(waveform(rows[0].width, level, phase)).alignment(Alignment::Center),
        Rect { y: rows[0].y + rows[0].height / 2, height: 1, ..rows[0] },
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Speak now \u{2014} Bishop is listening",
            Style::default().fg(PHOSPHOR),
        )))
        .alignment(Alignment::Center),
        rows[1],
    );
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "[ F9 ] send   \u{00b7}   [ Esc ] cancel",
            Style::default().fg(PHOSPHOR_DIM),
        )))
        .alignment(Alignment::Center),
        rows[2],
    );
}

/// A row of bar glyphs whose heights follow the live mic `level`, animated by
/// `phase` so it shimmers like a real meter.
fn waveform(width: u16, level: f32, phase: f64) -> Line<'static> {
    const BARS: [char; 8] =
        ['\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}'];
    let n = (width.saturating_sub(2)).clamp(8, 40) as usize;
    let mut s = String::with_capacity(n);
    for i in 0..n {
        // Per-column sine variation scaled by the smoothed input level.
        let wobble = (phase * 5.0 + i as f64 * 0.7).sin() * 0.5 + 0.5;
        let h = (level as f64 * (0.35 + 0.65 * wobble)).clamp(0.0, 1.0);
        let idx = ((h * (BARS.len() - 1) as f64).round() as usize).min(BARS.len() - 1);
        s.push(BARS[idx]);
    }
    Line::from(Span::styled(s, Style::default().fg(AMBER)))
}

/// A `Rect` centered in `area` at the given width/height percentages.
fn centered_rect(pct_x: u16, pct_y: u16, area: Rect) -> Rect {
    let w = area.width * pct_x / 100;
    let h = area.height * pct_y / 100;
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

fn render_about(f: &mut Frame, app: &App) {
    let body = framed(f, app, "[ Esc ] back to menu");
    about::view::render(f, body);
}

fn render_config(f: &mut Frame, app: &App) {
    let body = framed(
        f,
        app,
        "[ Up/Down ] select    [ Left/Right ] adjust    [ Esc ] save & menu",
    );
    crate::config::view::render(f, body, &app.settings, &app.config);
}

fn render_alerts(f: &mut Frame, app: &App) {
    let body = framed(
        f,
        app,
        "[ Up/Down ] select   [ Enter ] mute/unmute   [ T ] test   [ X ] delete   [ Esc ] menu",
    );
    let targets = app.notification.lock().unwrap().clone();
    crate::notification::view::render(f, body, &targets, &app.alerts);
}

fn render_map(f: &mut Frame, app: &App) {
    let hint = match app.map.mode {
        crate::map::MapMode::Browse => {
            "[N]add [R]ename [M]ove [S]ize [C]ams [X]del   [Up/Down] select   [Esc] menu"
        }
        crate::map::MapMode::Move => "[ Arrows ] move room    [ Enter/Esc ] done",
        crate::map::MapMode::Resize => "[ Arrows ] resize room    [ Enter/Esc ] done",
        crate::map::MapMode::Rename => "[ Type ] name    [ Enter ] save    [ Esc ] cancel",
        crate::map::MapMode::Cameras => "[ Up/Down ] camera    [ Space/Enter ] assign    [ Esc ] back",
    };
    let body = framed(f, app, hint);
    crate::map::view::render(f, body, &app.map, &app.scan);
}
