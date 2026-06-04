//! Rendering for the WORKFLOWS screen: the selected graph's node canvas on the
//! left and a context panel (graph list, selected-node detail, palette, or the
//! config form) on the right. Mirrors `map::view`.
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols::Marker,
    text::{Line, Span},
    widgets::{
        canvas::{Canvas, Line as CanvasLine, Rectangle as CanvasRect},
        Block, BorderType, Borders, Paragraph,
    },
    Frame,
};

use super::domain::{
    ConcurrencyPolicy, GraphNode, NodeKind, Trigger, WfFieldKind, WfMode, WorkflowsState, NODE_H,
    NODE_W, WF_GRID_H, WF_GRID_W,
};
use super::engine::WorkflowEngine;
use crate::scan::ScanState;
use crate::ui::chrome::panel;
use crate::ui::theme::{AMBER, PHOSPHOR, PHOSPHOR_DIM, PHOSPHOR_FAINT};

pub fn render(
    f: &mut Frame,
    area: Rect,
    state: &WorkflowsState,
    scan: &ScanState,
    engine: &WorkflowEngine,
) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(area);
    render_canvas(f, cols[0], state);
    render_context(f, cols[1], state, scan, engine);
    let _ = scan;
}

/// The left canvas: a faint grid, the edges between node centres, the node
/// rectangles (y-flipped so a y-down grid reads right-side up), and each node's
/// kind label.
fn render_canvas(f: &mut Frame, area: Rect, state: &WorkflowsState) {
    let gw = WF_GRID_W as f64;
    let gh = WF_GRID_H as f64;
    let (vx, vy, vw, vh) = state.viewport();
    // Grid spacing scales with the zoom span so a ~8×6 lattice stays visible at
    // every level (a fixed step vanishes when zoomed in past it).
    let stepx = ((vw / 8).max(1)) as usize;
    let stepy = ((vh / 6).max(1)) as usize;

    let Some(graph) = state.graph() else {
        let canvas = Canvas::default()
            .block(panel(" GRAPH ", false))
            .marker(Marker::Braille)
            .x_bounds([0.0, gw])
            .y_bounds([0.0, gh])
            .paint(|_| {});
        f.render_widget(canvas, area);
        let note = Line::from(Span::styled(
            "NO GRAPHS  \u{00b7}  PRESS [G] THEN [N]",
            Style::default().fg(PHOSPHOR_DIM),
        ));
        let mid = Rect { x: area.x + 2, y: area.y + area.height / 2, width: area.width.saturating_sub(4), height: 1 };
        f.render_widget(Paragraph::new(note).alignment(Alignment::Center), mid);
        return;
    };

    let nodes = graph.nodes.clone();
    let edges = graph.edges.clone();
    let selected = state.selected;
    let link_from = state.link_from.map(|(id, _)| id);
    let nodes_empty = nodes.is_empty();

    let canvas = Canvas::default()
        .block(panel(" GRAPH ", false))
        .marker(Marker::Braille)
        .x_bounds([vx as f64, (vx + vw) as f64])
        .y_bounds([(WF_GRID_H - (vy + vh)) as f64, (WF_GRID_H - vy) as f64])
        .paint(move |ctx| {
            // Faint grid — step scales with zoom so the lattice is visible at any level.
            for gx in (0..=WF_GRID_W).step_by(stepx) {
                ctx.draw(&CanvasLine {
                    x1: gx as f64, y1: 0.0, x2: gx as f64, y2: gh, color: PHOSPHOR_FAINT,
                });
            }
            for gy in (0..=WF_GRID_H).step_by(stepy) {
                ctx.draw(&CanvasLine {
                    x1: 0.0, y1: gy as f64, x2: gw, y2: gy as f64, color: PHOSPHOR_FAINT,
                });
            }
            ctx.layer();

            // Port marker positions (world coords, y-down): inputs sit on the left
            // edge, outputs on the right edge, spread down the node's height.
            let in_pos = |n: &GraphNode, idx: usize, cnt: usize| -> (f64, f64) {
                let cnt = cnt.max(1) as f64;
                (n.x as f64, n.y as f64 + (idx as f64 + 1.0) * NODE_H as f64 / (cnt + 1.0))
            };
            let out_pos = |n: &GraphNode, idx: usize, cnt: usize| -> (f64, f64) {
                let cnt = cnt.max(1) as f64;
                ((n.x + NODE_W) as f64, n.y as f64 + (idx as f64 + 1.0) * NODE_H as f64 / (cnt + 1.0))
            };

            // Edges: from the source's OUTPUT circle to the target's INPUT square.
            for e in &edges {
                let (Some(from), Some(to)) =
                    (nodes.iter().find(|n| n.id == e.from), nodes.iter().find(|n| n.id == e.to))
                else {
                    continue;
                };
                let oi = from.kind.output_ports().iter().position(|p| p.name == e.from_port.as_str());
                let ii = to.kind.input_ports().iter().position(|p| p.name == e.to_port.as_str());
                let (Some(oi), Some(ii)) = (oi, ii) else { continue };
                let (fx, fy) = out_pos(from, oi, from.kind.output_ports().len());
                let (tx, ty) = in_pos(to, ii, to.kind.input_ports().len());
                ctx.draw(&CanvasLine {
                    x1: fx,
                    y1: gh - fy,
                    x2: tx,
                    y2: gh - ty,
                    color: Color::Cyan,
                });
            }
            ctx.layer();

            // Node boxes (y-flipped; selected lit amber, source-of-link in amber too).
            for (i, n) in nodes.iter().enumerate() {
                let is_sel = i == selected;
                let is_src = link_from == Some(n.id);
                let color = if is_sel || is_src { AMBER } else { PHOSPHOR_DIM };
                let (nx, ny, nwid, nhei) = n.rect();
                let fy = gh - (ny as f64 + nhei as f64);
                ctx.draw(&CanvasRect {
                    x: nx as f64, y: fy, width: nwid as f64, height: nhei as f64, color,
                });
                if is_sel {
                    ctx.draw(&CanvasRect {
                        x: nx as f64 + 0.15,
                        y: fy + 0.15,
                        width: (nwid as f64 - 0.3).max(0.1),
                        height: (nhei as f64 - 0.3).max(0.1),
                        color: AMBER,
                    });
                }
            }
            ctx.layer();

            // Port markers: input squares (■) on the left, output circles (●) on
            // the right — lit amber on the selected / link-source node.
            for (i, n) in nodes.iter().enumerate() {
                let mc = if i == selected || link_from == Some(n.id) { AMBER } else { PHOSPHOR };
                let ins = n.kind.input_ports();
                for idx in 0..ins.len() {
                    let (wx, wy) = in_pos(n, idx, ins.len());
                    ctx.print(wx, gh - wy, Line::from(Span::styled("\u{25A0}", Style::default().fg(mc))));
                }
                let outs = n.kind.output_ports();
                for idx in 0..outs.len() {
                    let (wx, wy) = out_pos(n, idx, outs.len());
                    ctx.print(wx, gh - wy, Line::from(Span::styled("\u{25CF}", Style::default().fg(mc))));
                }
            }
            ctx.layer();

            // Kind labels, centred in each box.
            for (i, n) in nodes.iter().enumerate() {
                let (cx, cy_grid) = n.center();
                let cy = gh - cy_grid as f64;
                let color = if i == selected || link_from == Some(n.id) { AMBER } else { PHOSPHOR };
                let label = n.kind.label();
                ctx.print(
                    cx as f64 - label.chars().count() as f64 / 2.0,
                    cy,
                    Line::from(Span::styled(label.to_string(), Style::default().fg(color))),
                );
            }
        });
    f.render_widget(canvas, area);

    if nodes_empty {
        let note = Line::from(Span::styled(
            "NO NODES  \u{00b7}  PRESS [A] TO ADD",
            Style::default().fg(PHOSPHOR_DIM),
        ));
        let mid = Rect { x: area.x + 2, y: area.y + area.height / 2, width: area.width.saturating_sub(4), height: 1 };
        f.render_widget(Paragraph::new(note).alignment(Alignment::Center), mid);
    }
}

/// The right panel: graph list + node detail, or the palette / config form.
fn render_context(
    f: &mut Frame,
    area: Rect,
    state: &WorkflowsState,
    scan: &ScanState,
    engine: &WorkflowEngine,
) {
    let outer = panel(" WORKFLOW ", true);
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(inner);

    match state.mode {
        WfMode::Palette => render_palette(f, rows[0], state),
        WfMode::Config => render_config_form(f, rows[0], state),
        WfMode::Graphs => render_graphs(f, rows[0], state),
        WfMode::Link => render_link(f, rows[0], state),
        _ => render_detail(f, rows[0], state, scan),
    }

    if !state.status.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(state.status.clone(), Style::default().fg(AMBER))))
                .alignment(Alignment::Center),
            rows[1],
        );
    }
    let _ = engine;
}

/// A one-line summary of a graph's trigger, for the list + detail views.
fn trigger_summary(t: &Trigger) -> String {
    match t {
        Trigger::Manual => "manual".into(),
        Trigger::Cron { every_secs } => format!("every {every_secs}s"),
        Trigger::OnNewAlert { min_level } => format!("on alert \u{2265} {}", min_level.label()),
    }
}

fn concurrency_summary(c: &ConcurrencyPolicy) -> String {
    match c {
        ConcurrencyPolicy::SkipIfRunning => "skip if running".into(),
        ConcurrencyPolicy::QueueSingle { max_queued } => format!("queue {max_queued}"),
        ConcurrencyPolicy::Concurrent { max_concurrent } => format!("concurrent {max_concurrent}"),
    }
}

/// Selected-node detail plus the graph list and engine line.
fn render_detail(f: &mut Frame, area: Rect, state: &WorkflowsState, scan: &ScanState) {
    let mut lines: Vec<Line> = Vec::new();

    // Graph list (names + enabled + trigger).
    lines.push(Line::from(Span::styled("GRAPHS", Style::default().fg(PHOSPHOR_DIM))));
    if state.workflows.graphs.is_empty() {
        lines.push(Line::from(Span::styled(
            " (none)  \u{00b7}  [G] then [N]",
            Style::default().fg(PHOSPHOR_FAINT),
        )));
    }
    for (i, g) in state.workflows.graphs.iter().enumerate() {
        let active = i == state.workflows.selected;
        let mark = if g.enabled { "\u{25CF}" } else { "\u{25CB}" };
        let style = if active {
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(PHOSPHOR)
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {mark} "), Style::default().fg(if g.enabled { PHOSPHOR } else { PHOSPHOR_DIM })),
            Span::styled(g.name.clone(), style),
            Span::styled(format!("  ({})", trigger_summary(&g.trigger)), Style::default().fg(PHOSPHOR_FAINT)),
        ]));
    }
    lines.push(Line::from(""));

    // Selected node detail.
    if let Some(node) = state.selected_node() {
        lines.push(Line::from(vec![
            Span::styled("NODE  ", Style::default().fg(PHOSPHOR_DIM)),
            Span::styled(node.kind.label().to_string(), Style::default().fg(AMBER).add_modifier(Modifier::BOLD)),
        ]));
        // Inputs (with wired source / literal), then outputs.
        let graph = state.graph();
        if !node.kind.input_ports().is_empty() {
            lines.push(Line::from(Span::styled("INPUTS", Style::default().fg(PHOSPHOR_DIM))));
            for port in node.kind.input_ports() {
                let wired = graph
                    .and_then(|g| g.edges.iter().find(|e| e.to == node.id && e.to_port == port.name))
                    .and_then(|e| graph.and_then(|g| g.node(e.from)).map(|n| (n.kind.label(), e.from_port.clone())));
                let detail = match wired {
                    Some((label, fp)) => format!("\u{2190} {label}.{fp}"),
                    None => {
                        let lit = node.config.get(port.name).and_then(|v| v.as_str()).unwrap_or("");
                        if lit.trim().is_empty() {
                            if port.required { "(required)".into() } else { "(unset)".into() }
                        } else {
                            format!("= {lit}")
                        }
                    }
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("  {}  ", port.name), Style::default().fg(PHOSPHOR)),
                    Span::styled(detail, Style::default().fg(PHOSPHOR_FAINT)),
                ]));
            }
        }
        let outs: Vec<&str> = node.kind.output_ports().iter().map(|p| p.name).collect();
        if !outs.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("OUT   ", Style::default().fg(PHOSPHOR_DIM)),
                Span::styled(outs.join(", "), Style::default().fg(PHOSPHOR_FAINT)),
            ]));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "No node selected.  [A] add node",
            Style::default().fg(PHOSPHOR_FAINT),
        )));
    }

    // Engine line: trigger + concurrency for the active graph.
    if let Some(g) = state.graph() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("TRIG  ", Style::default().fg(PHOSPHOR_DIM)),
            Span::styled(trigger_summary(&g.trigger), Style::default().fg(PHOSPHOR)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("CONC  ", Style::default().fg(PHOSPHOR_DIM)),
            Span::styled(concurrency_summary(&g.concurrency), Style::default().fg(PHOSPHOR)),
        ]));
        let topo = g.topo_order().map(|_| "ok".to_string()).unwrap_or_else(|e| e);
        lines.push(Line::from(vec![
            Span::styled("DAG   ", Style::default().fg(PHOSPHOR_DIM)),
            Span::styled(format!("{} node(s) \u{00b7} {topo}", g.nodes.len()), Style::default().fg(PHOSPHOR_FAINT)),
        ]));
    }

    f.render_widget(Paragraph::new(lines), area);
    let _ = scan;
}

/// Link-mode picker: first the source output port (▸ = the one `←/→` selects),
/// then after lock-in the target node + input port, with a type-compat hint.
fn render_link(f: &mut Frame, area: Rect, state: &WorkflowsState) {
    let ty_str = |t| format!("{t:?}").to_lowercase();
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(Span::styled("LINK", Style::default().fg(PHOSPHOR_DIM))));
    lines.push(Line::from(""));

    match state.link_from {
        None => {
            // Phase 1: choose the source output port on the selected node.
            let Some(src) = state.selected_node() else {
                lines.push(Line::from(Span::styled(
                    "No source node selected.",
                    Style::default().fg(PHOSPHOR_FAINT),
                )));
                f.render_widget(Paragraph::new(lines), area);
                return;
            };
            lines.push(Line::from(vec![
                Span::styled("FROM  ", Style::default().fg(PHOSPHOR_DIM)),
                Span::styled(
                    src.kind.label().to_string(),
                    Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
                ),
            ]));
            lines.push(Line::from(Span::styled("OUTPUT", Style::default().fg(PHOSPHOR_DIM))));
            for (i, port) in src.kind.output_ports().iter().enumerate() {
                let sel = i == state.link_out;
                let style = if sel {
                    Style::default().fg(AMBER).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(PHOSPHOR)
                };
                lines.push(Line::from(vec![
                    Span::styled(if sel { "\u{25B8} " } else { "  " }, style),
                    Span::styled(port.name.to_string(), style),
                    Span::styled(format!("  : {}", ty_str(port.ty)), Style::default().fg(PHOSPHOR_FAINT)),
                ]));
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "\u{2190}/\u{2192} output \u{00b7} Enter \u{2192} pick target",
                Style::default().fg(PHOSPHOR_FAINT),
            )));
        }
        Some((from_id, out_idx)) => {
            // Phase 2: source port is locked; choose the target node + input port.
            let src_port = state
                .graph()
                .and_then(|g| g.node(from_id))
                .and_then(|n| n.kind.output_ports().get(out_idx).copied());
            if let (Some(src), Some(sp)) = (
                state.graph().and_then(|g| g.node(from_id)),
                src_port,
            ) {
                lines.push(Line::from(vec![
                    Span::styled("FROM  ", Style::default().fg(PHOSPHOR_DIM)),
                    Span::styled(
                        format!("{}.{}", src.kind.label(), sp.name),
                        Style::default().fg(PHOSPHOR),
                    ),
                ]));
            }
            let Some(tgt) = state.selected_node() else {
                f.render_widget(Paragraph::new(lines), area);
                return;
            };
            lines.push(Line::from(vec![
                Span::styled("TO    ", Style::default().fg(PHOSPHOR_DIM)),
                Span::styled(
                    tgt.kind.label().to_string(),
                    Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
                ),
            ]));
            if tgt.kind.input_ports().is_empty() {
                lines.push(Line::from(Span::styled(
                    "  (this node has no inputs)",
                    Style::default().fg(PHOSPHOR_FAINT),
                )));
            } else {
                lines.push(Line::from(Span::styled("INPUT", Style::default().fg(PHOSPHOR_DIM))));
                for (i, port) in tgt.kind.input_ports().iter().enumerate() {
                    let sel = i == state.link_in;
                    let compat = src_port.map_or(true, |sp| sp.ty == port.ty);
                    let style = if sel {
                        Style::default()
                            .fg(if compat { AMBER } else { Color::Red })
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(PHOSPHOR)
                    };
                    let mut spans = vec![
                        Span::styled(if sel { "\u{25B8} " } else { "  " }, style),
                        Span::styled(port.name.to_string(), style),
                        Span::styled(format!("  : {}", ty_str(port.ty)), Style::default().fg(PHOSPHOR_FAINT)),
                    ];
                    if !compat {
                        spans.push(Span::styled("  \u{2717} type", Style::default().fg(Color::Red)));
                    }
                    lines.push(Line::from(spans));
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "\u{2191}/\u{2193} target \u{00b7} \u{2190}/\u{2192} input \u{00b7} Enter \u{2192} link",
                Style::default().fg(PHOSPHOR_FAINT),
            )));
        }
    }
    f.render_widget(Paragraph::new(lines), area);
}

/// The node-kind checklist used in Palette mode; the cursor row is a solid amber
/// panel (mirrors `map`'s camera picker).
fn render_palette(f: &mut Frame, area: Rect, state: &WorkflowsState) {
    let kinds = NodeKind::ALL;
    let mut constraints = vec![Constraint::Length(1)];
    for _ in kinds.iter() {
        constraints.push(Constraint::Length(3));
        constraints.push(Constraint::Length(1));
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    for (i, k) in kinds.iter().enumerate() {
        let Some(&box_area) = rows.get(1 + i * 2) else { break };
        let selected = i == state.palette_cursor;
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(if selected { BorderType::Double } else { BorderType::Plain });
        let (block, label_style) = if selected {
            (
                block.border_style(Style::default().fg(AMBER)).style(Style::default().bg(AMBER).fg(Color::Black)),
                Style::default().fg(Color::Black).add_modifier(Modifier::BOLD),
            )
        } else {
            (block.border_style(Style::default().fg(PHOSPHOR_DIM)), Style::default().fg(PHOSPHOR))
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(k.label(), label_style)))
                .alignment(Alignment::Center)
                .block(block),
            box_area,
        );
    }
}

/// The per-node + graph config form; the focused row is highlighted amber.
fn render_config_form(f: &mut Frame, area: Rect, state: &WorkflowsState) {
    let fields = state.config_fields();
    let node = state.selected_node();
    let graph = state.graph();
    let mut lines: Vec<Line> = Vec::new();

    if let Some(n) = node {
        lines.push(Line::from(vec![
            Span::styled("NODE  ", Style::default().fg(PHOSPHOR_DIM)),
            Span::styled(n.kind.label().to_string(), Style::default().fg(AMBER).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(""));
    }

    for (i, field) in fields.iter().enumerate() {
        let focused = i == state.config_cursor;
        let value = match field.kind {
            WfFieldKind::Text => {
                let raw = node
                    .and_then(|n| n.config.get(field.key))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if focused {
                    format!("{raw}\u{2588}")
                } else {
                    raw
                }
            }
            WfFieldKind::Level => node
                .and_then(|n| n.config.get("level"))
                .and_then(|v| v.as_str())
                .unwrap_or("info")
                .to_ascii_uppercase(),
            WfFieldKind::Trigger => graph.map(|g| trigger_summary(&g.trigger)).unwrap_or_default(),
            WfFieldKind::Concurrency => {
                graph.map(|g| concurrency_summary(&g.concurrency)).unwrap_or_default()
            }
        };
        let label_style = if focused {
            Style::default().fg(AMBER).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(PHOSPHOR_DIM)
        };
        let value_style = if focused {
            Style::default().fg(AMBER)
        } else {
            Style::default().fg(PHOSPHOR)
        };
        let arrows = if focused && field.kind != WfFieldKind::Text { " \u{2190}\u{2192}" } else { "" };
        lines.push(Line::from(vec![
            Span::styled(format!("{:<12}", field.label), label_style),
            Span::styled(value, value_style),
            Span::styled(arrows.to_string(), Style::default().fg(PHOSPHOR_FAINT)),
        ]));
    }

    f.render_widget(Paragraph::new(lines), area);
}

/// The graph switcher list; the cursor row is a solid amber panel.
fn render_graphs(f: &mut Frame, area: Rect, state: &WorkflowsState) {
    let mut lines: Vec<Line> = Vec::new();

    if state.graphs_editing {
        let tag = if state.graphs_rename { "RENAME" } else { "NEW GRAPH" };
        lines.push(Line::from(Span::styled(tag, Style::default().fg(PHOSPHOR_DIM))));
        lines.push(Line::from(vec![
            Span::styled("NAME  ", Style::default().fg(PHOSPHOR_DIM)),
            Span::styled(
                format!("{}\u{2588}", state.graphs_buffer),
                Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
            ),
        ]));
        f.render_widget(Paragraph::new(lines), area);
        return;
    }

    if state.workflows.graphs.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no graphs)  \u{00b7}  [N] new",
            Style::default().fg(PHOSPHOR_FAINT),
        )));
        f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);
        return;
    }

    for (i, g) in state.workflows.graphs.iter().enumerate() {
        let cursor = i == state.graphs_cursor;
        let active = i == state.workflows.selected;
        let mark = if g.enabled { "\u{25CF}" } else { "\u{25CB}" };
        let active_tag = if active { " *" } else { "  " };
        let label =
            format!("{mark}{active_tag} {}  ({}, {})", g.name, trigger_summary(&g.trigger), g.nodes.len());
        let style = if cursor {
            Style::default().fg(Color::Black).bg(AMBER).add_modifier(Modifier::BOLD)
        } else if active {
            Style::default().fg(AMBER)
        } else {
            Style::default().fg(PHOSPHOR)
        };
        lines.push(Line::from(Span::styled(label, style)));
    }

    f.render_widget(Paragraph::new(lines), area);
}
