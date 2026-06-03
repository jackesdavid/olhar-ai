//! The ALERTS screen: a simple manager for trusted notification targets. Targets
//! are added by pairing from Telegram (`/pair <code>`); here the operator can see
//! them, toggle delivery on/off, remove one, or fire a test alert. Drawn as
//! stacked boxes to match the home menu and device list.
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph},
    Frame,
};

use super::domain::{NotificationConfig, NotificationTarget};
use super::handler::AlertsScreen;
use crate::ui::chrome::panel;
use crate::ui::theme::{AMBER, PHOSPHOR, PHOSPHOR_DIM, PHOSPHOR_FAINT};

/// Render the targets list (a snapshot, cloned by the caller) plus a status line.
pub fn render(f: &mut Frame, area: Rect, targets: &[NotificationTarget], screen: &AlertsScreen) {
    let outer = panel(" ALERT TARGETS ", true);
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    if targets.is_empty() {
        render_empty(f, inner);
        return;
    }

    // Reserve the bottom row for a status line; the list fills the rest.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);
    render_list(f, rows[0], targets, screen);

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

fn render_list(f: &mut Frame, area: Rect, targets: &[NotificationTarget], screen: &AlertsScreen) {
    // top pad, then [box(3), blurb(1), gap(1)] per target — matching the menu.
    let mut constraints = vec![Constraint::Length(1)];
    for _ in targets {
        constraints.push(Constraint::Length(3));
        constraints.push(Constraint::Length(1));
        constraints.push(Constraint::Length(1));
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    for (i, t) in targets.iter().enumerate() {
        let selected = i == screen.selected;
        let box_area = rows[1 + i * 3];
        let blurb_area = rows[2 + i * 3];

        let mark = if t.enabled { "\u{25CF}" } else { "\u{25CB}" }; // ● online / ○ muted
        let label = format!("{:02}  {} {}", i + 1, mark, t.label);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(if selected { BorderType::Double } else { BorderType::Plain });
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

        let state = if t.enabled { "ONLINE" } else { "MUTED" };
        let blurb = format!("chat {}  \u{00b7}  {}", t.chat_id, state);
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

/// Empty state: explain how to pair a phone, showing the live `/pair` code when
/// one is configured.
fn render_empty(f: &mut Frame, area: Rect) {
    let pair_line = match NotificationConfig::from_env() {
        Some(cfg) if cfg.can_pair() => {
            format!("From Telegram, send the bot:  /pair {}", cfg.pairing_code)
        }
        Some(_) => "Set TELEGRAM_PAIRING_CODE, then /pair it from Telegram.".to_string(),
        None => "Set TELEGRAM_BOT_TOKEN and TELEGRAM_PAIRING_CODE to enable alerts.".to_string(),
    };
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "NO TARGETS PAIRED",
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(pair_line, Style::default().fg(PHOSPHOR_DIM))),
        Line::from(Span::styled(
            "A phone becomes a target once it sends the code.",
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
