//! Rendering for the home menu: a column of bordered option boxes on the left
//! (Nostromo-style — the selected one is a filled amber panel) and a helper
//! panel describing the highlighted entry on the right.
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{canvas::Canvas, Block, BorderType, Borders, Paragraph, Wrap},
    Frame,
};

use super::domain::Menu;
use crate::app::domain::Screen;
use crate::ui::chrome::panel;
use crate::ui::theme::{AMBER, PHOSPHOR, PHOSPHOR_DIM, PHOSPHOR_FAINT};
use crate::ui::wireframe;

/// The left-hand list of features, drawn as stacked boxes. The selected box is
/// a solid amber panel with a double border; the rest are calm phosphor
/// outlines.
pub fn render_list(f: &mut Frame, area: Rect, menu: &Menu) {
    let outer = panel(" MAIN MENU ", true);
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    // top pad, then [box(3), blurb(1), gap(1)] per item.
    let mut constraints = vec![Constraint::Length(1)];
    for _ in &menu.items {
        constraints.push(Constraint::Length(3));
        constraints.push(Constraint::Length(1));
        constraints.push(Constraint::Length(1));
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    for (i, item) in menu.items.iter().enumerate() {
        let selected = i == menu.selected;
        let box_area = rows[1 + i * 3];
        let blurb_area = rows[2 + i * 3];

        let label = format!("{:02}   {}", i + 1, item.label);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(if selected {
                BorderType::Double
            } else {
                BorderType::Plain
            });
        let (block, label_style) = if selected {
            (
                // Filled amber panel with black text — the lit selection.
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

        let blurb_style = if selected {
            Style::default().fg(AMBER)
        } else {
            Style::default().fg(PHOSPHOR_FAINT)
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(item.blurb, blurb_style)))
                .alignment(Alignment::Center),
            blurb_area,
        );
    }
}

/// The wireframe schematic viewport: a braille-canvas 3D animation themed to the
/// highlighted menu entry and spun by the global sweep `phase`. Sits above the
/// briefing panel on the home screen.
pub fn render_visual(f: &mut Frame, area: Rect, menu: &Menu, phase: f64) {
    // Key the schematic off the entry's target screen (not its row index) so the
    // shapes stay correct no matter how the menu is ordered.
    let target = menu.selected_item().target;
    let canvas = Canvas::default()
        .block(panel(" VISUAL ", false))
        .marker(Marker::Braille)
        .x_bounds([-1.0, 1.0])
        .y_bounds([-1.0, 1.0])
        .paint(move |ctx| match target {
            Screen::Scan => wireframe::draw_globe(ctx, phase),
            Screen::Chat => wireframe::draw_scope(ctx, phase),
            Screen::Config => wireframe::draw_torus(ctx, phase),
            Screen::Alerts => wireframe::draw_beacon(ctx, phase),
            _ => wireframe::draw_ship(ctx, phase), // About
        });
    f.render_widget(canvas, area);
}

/// The right-hand helper panel describing the currently highlighted entry.
pub fn render_help(f: &mut Frame, area: Rect, menu: &Menu) {
    let item = menu.selected_item();
    let mut lines: Vec<Line> = vec![Line::from("")];
    for (i, raw) in item.help.iter().enumerate() {
        // First line is the heading — render it amber/bold.
        let style = if i == 0 {
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(PHOSPHOR)
        };
        lines.push(Line::from(Span::styled(format!(" {raw}"), style)));
    }
    f.render_widget(
        Paragraph::new(lines)
            .block(panel(" BRIEFING ", false))
            .wrap(Wrap { trim: false }),
        area,
    );
}
