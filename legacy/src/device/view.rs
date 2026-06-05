//! Rendering for the device-detail screen: the controls stacked vertically on
//! the left, with the camera viewport on the right to use the space.
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

use super::domain::{DeviceDetail, Field};
use crate::scan::Device;
use crate::stream::LiveStream;
use crate::ui::chrome::panel;
use crate::ui::theme::{AMBER, PHOSPHOR, PHOSPHOR_DIM, PHOSPHOR_FAINT};

pub fn render(
    f: &mut Frame,
    area: Rect,
    detail: &DeviceDetail,
    device: &Device,
    live: Option<&LiveStream>,
) {
    // Form (vertical) on the left; viewport on the right to use the space.
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // device info
            Constraint::Length(3), // name
            Constraint::Length(3), // username
            Constraint::Length(3), // password
            Constraint::Length(3), // action buttons
            Constraint::Min(0),    // status
        ])
        .split(cols[0]);

    // ---- device info ------------------------------------------------------
    let creds = if device.has_credentials() {
        Span::styled("CREDENTIALS SAVED", Style::default().fg(PHOSPHOR))
    } else {
        Span::styled("NO CREDENTIALS", Style::default().fg(PHOSPHOR_DIM))
    };
    let info = vec![
        Line::from(vec![
            Span::styled("IP    ", Style::default().fg(PHOSPHOR_DIM)),
            Span::styled(device.ip.clone(), Style::default().fg(PHOSPHOR)),
            Span::styled("   ", Style::default()),
            creds,
        ]),
        Line::from(vec![
            Span::styled("MODEL ", Style::default().fg(PHOSPHOR_DIM)),
            Span::styled(
                device.model.clone().or_else(|| device.manufacturer.clone()).unwrap_or_else(|| "—".into()),
                Style::default().fg(PHOSPHOR),
            ),
        ]),
    ];
    f.render_widget(
        Paragraph::new(info).block(panel(" DEVICE ", true)).wrap(Wrap { trim: true }),
        rows[0],
    );

    // ---- editable name + credential fields --------------------------------
    input_field(f, rows[1], " NAME ", &detail.name, false, detail.field == Field::Name);
    input_field(f, rows[2], " USERNAME ", &detail.username, false, detail.field == Field::Username);
    input_field(f, rows[3], " PASSWORD ", &detail.password, true, detail.field == Field::Password);

    // ---- action buttons ---------------------------------------------------
    let btns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(26),
            Constraint::Percentage(20),
            Constraint::Percentage(20),
            Constraint::Percentage(14),
            Constraint::Percentage(20),
        ])
        .split(rows[4]);
    button(f, btns[0], "TEST & SAVE", detail.field == Field::Test);
    button(f, btns[1], "SNAPSHOT", detail.field == Field::Snapshot);
    button(f, btns[2], "DESCRIBE", detail.field == Field::Describe);
    button(f, btns[3], "LIVE", detail.field == Field::Live);
    button(f, btns[4], "RENAME", detail.field == Field::Rename);

    // ---- status -----------------------------------------------------------
    let status = if detail.status.is_empty() {
        "Tab between fields. Credentials save only after a successful test.".to_string()
    } else {
        detail.status.clone()
    };
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(status, Style::default().fg(AMBER))))
            .block(panel(" STATUS ", false))
            .wrap(Wrap { trim: true }),
        rows[5],
    );

    // ---- viewport (snapshot / live) on the right --------------------------
    render_viewport(f, cols[1], detail, live);
}

fn input_field(f: &mut Frame, area: Rect, title: &'static str, value: &str, masked: bool, focused: bool) {
    let shown: String = if masked {
        value.chars().map(|_| '\u{2022}').collect()
    } else {
        value.to_string()
    };
    let border = if focused { AMBER } else { PHOSPHOR_DIM };
    let mut spans = vec![Span::styled(shown, Style::default().fg(PHOSPHOR))];
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

/// The right column: the live feed for this device if streaming, otherwise the
/// last snapshot, otherwise a hint.
fn render_viewport(f: &mut Frame, area: Rect, detail: &DeviceDetail, live: Option<&LiveStream>) {
    let block = panel(" VIEWPORT ", false);
    let inner = block.inner(area);
    f.render_widget(block, area);

    // A live feed for THIS device takes priority over the static snapshot.
    if let Some(s) = live {
        if s.device_ip == detail.ip {
            crate::stream::view::render_feed(f, inner, s);
            return;
        }
    }

    match &detail.image {
        Some(img) => img.render(f, inner),
        None => {
            let note = Line::from(Span::styled(
                "NO IMAGE  ·  focus SNAPSHOT or LIVE and press [Enter]",
                Style::default().fg(PHOSPHOR_FAINT),
            ));
            let mid = Rect {
                x: inner.x,
                y: inner.y + inner.height / 2,
                width: inner.width,
                height: 1,
            };
            f.render_widget(Paragraph::new(note).alignment(Alignment::Center), mid);
        }
    }
}

