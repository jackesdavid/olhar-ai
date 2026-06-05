//! Rendering for the chat: the scrollback log and the inquiry input box.
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

use super::domain::Conversation;
use crate::ui::chrome::panel;
use crate::ui::theme::{AMBER, PHOSPHOR, PHOSPHOR_DIM};

pub fn render_log(f: &mut Frame, area: Rect, convo: &Conversation) {
    let mut lines: Vec<Line> = convo.log.clone();
    if let Some(t) = &convo.typing {
        // A reply typing out: revealed text so far, plus a block cursor.
        let shown: String = t.full.chars().take(t.shown).collect();
        lines.push(Line::from(vec![
            Span::styled("BISHOP: ", Style::default().fg(AMBER)),
            Span::styled(shown, Style::default().fg(PHOSPHOR)),
            Span::styled("\u{2588}", Style::default().fg(PHOSPHOR)),
        ]));
    } else if convo.pending {
        // Awaiting Claude: a transient "working" line.
        lines.push(Line::from(vec![
            Span::styled("BISHOP: ", Style::default().fg(AMBER)),
            Span::styled("\u{2588} working\u{2026}", Style::default().fg(PHOSPHOR_DIM)),
        ]));
    }

    let visible = area.height.saturating_sub(2) as usize;
    let start = lines.len().saturating_sub(visible);
    let view: Vec<Line> = lines[start..].to_vec();
    f.render_widget(
        Paragraph::new(view)
            .block(panel(" LOG ", false))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub fn render_input(f: &mut Frame, area: Rect, convo: &Conversation) {
    // Render the input with the cursor at its position: text before, the char
    // under the cursor as a reverse-video block, then text after. At the end of
    // the line the cursor is a solid block.
    let cursor = convo.cursor.min(convo.input.len());
    let before = &convo.input[..cursor];
    let after = &convo.input[cursor..];
    let mut spans = vec![
        Span::styled(" > ", Style::default().fg(AMBER)),
        Span::styled(before.to_string(), Style::default().fg(PHOSPHOR)),
    ];
    match after.chars().next() {
        Some(c) => {
            spans.push(Span::styled(
                c.to_string(),
                Style::default().fg(PHOSPHOR).add_modifier(Modifier::REVERSED),
            ));
            spans.push(Span::styled(
                after[c.len_utf8()..].to_string(),
                Style::default().fg(PHOSPHOR),
            ));
        }
        None => spans.push(Span::styled("\u{2588}", Style::default().fg(PHOSPHOR))),
    }
    let block = panel(" INQUIRY   [Enter] send   [Esc] menu ", true);
    f.render_widget(Paragraph::new(Line::from(spans)).block(block), area);
}
