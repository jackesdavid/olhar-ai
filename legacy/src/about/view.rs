//! Rendering for the About screen: the dossier table and flavor notes.
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

use super::domain::{DOSSIER, NOTES};
use crate::ui::chrome::panel;
use crate::ui::theme::{AMBER, PHOSPHOR, PHOSPHOR_DIM};

pub fn render(f: &mut Frame, area: Rect) {
    let mut lines: Vec<Line> = vec![Line::from("")];

    for (label, value) in DOSSIER {
        lines.push(Line::from(vec![
            Span::styled(format!("  {label}  "), Style::default().fg(PHOSPHOR_DIM)),
            Span::styled((*value).to_string(), Style::default().fg(PHOSPHOR)),
        ]));
    }

    lines.push(Line::from(""));
    for note in NOTES {
        lines.push(Line::from(Span::styled(
            format!("  {note}"),
            Style::default().fg(AMBER).add_modifier(Modifier::ITALIC),
        )));
    }

    f.render_widget(
        Paragraph::new(lines)
            .block(panel(" SYSTEM DOSSIER ", true))
            .wrap(Wrap { trim: false }),
        area,
    );
}
