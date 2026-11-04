//! Draws the editor: title, the flow view (or the older lineage view: inputs | node |
//! used by), status and key hints, plus the find and help overlays.

use conduit_core::{ParamSpec, ProjectNode, Reference, Rgba, TreeItem, parse_color, to_hex};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph};

use super::app::{App, Bar, ColorEdit, EditRow, Mode, Pane, Pick, Row, shown, uses};
use super::canvas;
use super::flow::InState;
use crate::run::{self, Event, Shown};

const DIM: Style = Style::new().fg(Color::DarkGray);
const ERROR: Style = Style::new().fg(Color::Red);
const ACCENT: Color = Color::Cyan;
/// Background of the selected row; distinct from `DIM` text so dim parts stay readable.
const SELECTED_BG: Color = Color::Indexed(237);

pub fn draw(frame: &mut Frame, app: &mut App) {
    let screen = frame.area();
    if app.flow_view {
        // The flow view paints its own black background (§5.1).
        frame.buffer_mut().set_style(screen, canvas::STANDARD);
        app.too_small = screen.width < canvas::MIN_WIDTH || screen.height < canvas::MIN_HEIGHT;
        if app.too_small {
            draw_too_small(frame, app, screen);
            return;
        }
        // §4.7: title, canvas, status line, blank, action bar (4 rows), blank.
        let [title, main, status, _, bar, _] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(4),
            Constraint::Length(1),
        ])
        .areas(screen);
        draw_title(frame, app, title);
        draw_flow(frame, app, main);
        draw_flow_status(frame, app, status);
        draw_bar(frame, &app.bar(), bar);
    } else {
        let [title, main, status, keys] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(screen);
        draw_title(frame, app, title);
        let [up, node, down] = lineage_areas(main, app.rows.len());
        draw_tree(frame, app, Pane::Up, up);
        draw_node(frame, app, node);
        draw_tree(frame, app, Pane::Down, down);
        draw_status(frame, app, status);
        draw_keys(frame, app, keys);
    }

    match &app.mode {
        Mode::Pick { .. } => draw_pick(frame, app),
        Mode::Prompt { .. } => draw_prompt(frame, app),
        Mode::Confirm { .. } => draw_confirm(frame, app),
        Mode::Help => draw_help(frame, if app.flow_view { FLOW_HELP } else { HELP }),
        Mode::Color(edit) => draw_color(frame, edit),
        Mode::Normal => {}
    }
}

/// The flow and its focus (§4), or a hint in an empty project (§4.8).
fn draw_flow(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(focus) = app.flow_focus.clone() else {
        let hint = ["This project has no nodes yet.", "Press a to add one."];
        let width = hint.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u16;
        let x = area.x + area.width.saturating_sub(width) / 2;
        let y = area.y + area.height.saturating_sub(hint.len() as u16) / 2;
        for (i, line) in hint.iter().enumerate() {
            frame
                .buffer_mut()
                .set_string(x, y + i as u16, line, canvas::STANDARD);
        }
        return;
    };
    if app.edit_view.is_some() {
        draw_edit_view(frame, app, area);
        return;
    }
    let picture = canvas::draw(&app.project, &app.flow, Some(&focus));
    let recentre = std::mem::take(&mut app.recentre);
    app.viewport = app.viewport.follow(
        &picture,
        &app.flow,
        &focus,
        area.width,
        area.height,
        recentre,
    );
    picture.blit(frame.buffer_mut(), area, app.viewport.x, app.viewport.y);
}

/// One line of the edit view, before the box width is known.
enum EditLine {
    Blank,
    Rule,
    /// Text inside the box.
    Text(Vec<Span<'static>>),
    /// An input row: the stub outside the box (`▣──`), then its label.
    In(Span<'static>, Vec<Span<'static>>),
    /// An output row: its label, right-aligned, then the stub outside the box (`──▣`).
    Out(Vec<Span<'static>>, Span<'static>),
}

fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(|s| s.content.chars().count()).sum()
}

/// The edit view (§7): the focused node enlarged, its rows in sections, its errors last.
fn draw_edit_view(frame: &mut Frame, app: &App, area: Rect) {
    use canvas::{DIM, ERROR, FOCUS, RELATED, STANDARD};
    let Some(node) = app.focused() else { return };
    let rows = app.edit_rows();
    let cursor = app.edit_view.clone();
    let errors = app.edit_errors();
    let in_width = (rows.iter())
        .filter_map(|r| match r {
            EditRow::In(r) => Some(r.label().chars().count()),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let param_width = (rows.iter())
        .filter_map(|r| match r {
            EditRow::Param(name) => Some(name.chars().count()),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let out_width = (rows.iter())
        .filter_map(|r| match r {
            EditRow::Out(r) => Some(r.port.chars().count()),
            _ => None,
        })
        .max()
        .unwrap_or(0);

    let mut lines = vec![EditLine::Blank];
    let mut focus_line = 0;
    let mut section = 0;
    for row in &rows {
        let focused = cursor.as_ref() == Some(row);
        // The label's style and the value's: focus green, the rest of its row magenta.
        let (label, value) = if focused {
            (RELATED, FOCUS)
        } else {
            (STANDARD, STANDARD)
        };
        let this_section = match row {
            EditRow::Name => 0,
            EditRow::In(_) => 1,
            EditRow::Param(_) => 2,
            EditRow::Colour { .. } => 3,
            EditRow::Out(_) => 4,
        };
        if this_section != section {
            lines.push(EditLine::Rule);
            section = this_section;
        }
        if focused {
            focus_line = lines.len();
        }
        lines.push(match row {
            EditRow::Name => EditLine::Text(vec![
                Span::styled(" name: ", label),
                Span::styled(node.id.clone(), value),
            ]),
            EditRow::In(r) => {
                let (stub, stub_style) = match &r.state {
                    InState::Connected(_) => ("▣──", STANDARD),
                    InState::Missing => ("✕──", ERROR),
                    InState::Slot | InState::Optional => ("   ", STANDARD),
                };
                let stub_style = if focused { FOCUS } else { stub_style };
                let name = format!("{:<in_width$}", r.label());
                let mut spans = match &r.state {
                    InState::Connected(source) => vec![
                        Span::styled(name, value),
                        Span::styled(format!(" ← {}", source.short()), label),
                    ],
                    InState::Missing => vec![
                        Span::styled(name, if focused { value } else { ERROR }),
                        Span::styled("  not connected", ERROR),
                    ],
                    InState::Slot => vec![
                        Span::styled(name, value),
                        Span::styled("  (append another)", DIM),
                    ],
                    InState::Optional => {
                        vec![Span::styled(name, value), Span::styled("  (optional)", DIM)]
                    }
                };
                spans.retain(|s| !s.content.is_empty());
                EditLine::In(Span::styled(stub, stub_style), spans)
            }
            EditRow::Param(name) => {
                let mut spans = vec![Span::styled(format!("  {name:<param_width$} = "), label)];
                spans.extend(param_value_spans(node, name, value));
                EditLine::Text(spans)
            }
            EditRow::Colour { param, key } => {
                let colour = node.params.get(param).and_then(|t| t.get(key));
                let mut spans = vec![Span::raw("  ")];
                match colour.and_then(color_value) {
                    Some(c) => {
                        spans.push(swatch(c));
                        spans.push(Span::styled(format!(" {}", to_hex(c)), value));
                    }
                    None => spans.push(Span::styled(colour.map(shown).unwrap_or_default(), ERROR)),
                }
                spans.push(Span::styled(format!("  {key}"), label));
                EditLine::Text(spans)
            }
            EditRow::Out(r) => {
                let mut spans = vec![Span::styled(format!("{:>out_width$}", r.port), value)];
                if let Some(user) = &r.user {
                    spans.push(Span::styled(format!(" → {}", user.node), label));
                }
                let stub = if r.user.is_some() { "──▣" } else { "─" };
                let stub_style = if focused { FOCUS } else { STANDARD };
                EditLine::Out(spans, Span::styled(stub, stub_style))
            }
        });
    }
    if !errors.is_empty() {
        lines.push(EditLine::Rule);
        for e in &errors {
            lines.push(EditLine::Text(vec![Span::styled(format!(" ✕ {e}"), ERROR)]));
        }
    }

    // Width: wide enough for every row, as in the stories (47), up to the screen's.
    let widest = lines
        .iter()
        .map(|l| match l {
            EditLine::Text(s) => spans_width(s) + 1,
            EditLine::In(_, s) => spans_width(s) + 3,
            EditLine::Out(s, _) => spans_width(s) + 3,
            _ => 0,
        })
        .max()
        .unwrap_or(0)
        .max(node.kind.as_deref().unwrap_or("?").chars().count() + 4);
    let max_inner = usize::from(area.width).saturating_sub(10).max(8);
    let inner = widest.max(47).min(max_inner);
    let total = inner + 2;
    let x = area.x
        + (area.width.saturating_sub(total as u16) / 2)
            .saturating_sub(1)
            .max(3);

    let border = if errors.is_empty() { STANDARD } else { ERROR };
    let kind = node.kind.clone().unwrap_or_else(|| "?".into());
    let top_rest = inner.saturating_sub(kind.chars().count() + 3);
    let mut screen: Vec<Line> = vec![Line::from(vec![
        Span::styled("╭─ ", border),
        Span::styled(kind, border),
        Span::styled(format!(" {}╮", "─".repeat(top_rest)), border),
    ])];
    let fit = |spans: Vec<Span<'static>>, width: usize| -> (Vec<Span<'static>>, usize) {
        // Cut a row that's too long for the box.
        let mut out = Vec::new();
        let mut used = 0;
        for span in spans {
            let room = width.saturating_sub(used);
            let text: String = span.content.chars().take(room).collect();
            used += text.chars().count();
            out.push(Span::styled(text, span.style));
        }
        (out, used)
    };
    for line in lines {
        let mut spans = Vec::new();
        match line {
            EditLine::Blank => {
                spans.push(Span::styled(format!("│{}│", " ".repeat(inner)), border));
            }
            EditLine::Rule => {
                spans.push(Span::styled(format!("│{}│", "─".repeat(inner)), border));
            }
            EditLine::Text(content) => {
                let (content, used) = fit(content, inner);
                spans.push(Span::styled("│", border));
                spans.extend(content);
                spans.push(Span::styled(
                    format!("{}│", " ".repeat(inner - used)),
                    border,
                ));
            }
            EditLine::In(stub, content) => {
                let (content, used) = fit(content, inner.saturating_sub(2));
                let style = stub.style;
                spans.push(Span::styled(format!("{stub}"), style));
                spans.push(Span::styled("┼", border));
                spans.push(Span::styled("─ ", style));
                spans.extend(content);
                spans.push(Span::styled(
                    format!("{}│", " ".repeat(inner.saturating_sub(2) - used)),
                    border,
                ));
            }
            EditLine::Out(content, stub) => {
                let (content, used) = fit(content, inner.saturating_sub(3));
                let style = stub.style;
                spans.push(Span::styled(
                    format!("│{}", " ".repeat(inner.saturating_sub(2) - used)),
                    border,
                ));
                spans.extend(content);
                spans.push(Span::styled(" ─", style));
                spans.push(Span::styled("┼", border));
                spans.push(stub);
            }
        }
        screen.push(Line::from(spans));
    }
    screen.push(Line::from(Span::styled(
        format!("╰{}╯", "─".repeat(inner)),
        border,
    )));

    // Scroll so the focused row shows (one blank row above the box).
    let room = usize::from(area.height.saturating_sub(1));
    let focus_screen = focus_line + 1;
    let skip = if screen.len() <= room {
        0
    } else {
        (focus_screen + 2)
            .saturating_sub(room)
            .min(screen.len() - room)
    };
    let buf = frame.buffer_mut();
    for (i, line) in screen.iter().skip(skip).take(room).enumerate() {
        let y = area.y + 1 + i as u16;
        // Input stubs start three columns left of the box.
        let starts_outside = line
            .spans
            .first()
            .is_some_and(|s| !s.content.starts_with(['│', '╭', '╰']));
        let left = if starts_outside { x - 3 } else { x };
        buf.set_line(left, y, line, area.right().saturating_sub(left));
    }
}

/// A param's value in the edit view: `██ white (default)`, `2.5`, `unset (default)`.
fn param_value_spans(node: &ProjectNode, name: &str, style: Style) -> Vec<Span<'static>> {
    let spec = node.spec.as_ref().and_then(|s| s.param(name));
    let (value, default) = match node.params.get(name) {
        Some(v) => (Some(v.clone()), false),
        None => (spec.and_then(|p| p.default_value()), true),
    };
    let mut spans = Vec::new();
    match value {
        Some(v) => {
            if let Some(c) = color_value(&v)
                .filter(|_| spec.is_some_and(|p| p.kind == conduit_core::ParamKind::Color))
            {
                spans.push(swatch(c));
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(shown(&v), style));
        }
        None if spec.is_some_and(|p| p.required) => {
            spans.push(Span::styled("missing", canvas::ERROR));
            return spans;
        }
        None => spans.push(Span::styled("unset", style)),
    }
    if default {
        spans.push(Span::styled(" (default)", canvas::DIM));
    }
    spans
}

/// The status line (§4.7), with the probe on the right.
fn draw_flow_status(frame: &mut Frame, app: &App, area: Rect) {
    let probe = app.probe.as_ref().map(|probe| {
        let follow = if app.probe_follow { " (follow)" } else { "" };
        format!("  probe: {probe}{follow} ")
    });
    let probe_width = probe.as_ref().map_or(0, |p| p.chars().count() as u16);
    let [area, probe_area] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(probe_width)]).areas(area);
    if let Some(text) = probe {
        frame.render_widget(Span::styled(text, canvas::STANDARD), probe_area);
    }
    let text = app.flow_status();
    let style = if text.starts_with(['✕', '✗']) {
        canvas::ERROR
    } else {
        canvas::STANDARD
    };
    frame.render_widget(Span::styled(format!(" {text}"), style), area);
}

/// Spans for `(e)dit` / `[a]dd` style text: bracketed keys in `key`, the rest in `rest`.
fn keyed_spans(text: &str, key: Style, rest: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut chars = text.char_indices().peekable();
    let mut start = 0;
    while let Some((i, c)) = chars.next() {
        let close = match c {
            '(' => ')',
            '[' => ']',
            _ => continue,
        };
        if let Some(end) = text[i + 1..].find(close).map(|e| i + 1 + e) {
            if start < i {
                spans.push(Span::styled(text[start..i].to_string(), rest));
            }
            spans.push(Span::styled(text[i..=end].to_string(), key));
            start = end + 1;
            while chars.peek().is_some_and(|(j, _)| *j <= end) {
                chars.next();
            }
        }
    }
    if start < text.len() {
        spans.push(Span::styled(text[start..].to_string(), rest));
    }
    spans
}

/// The action bar (§6.4): title rule, blank, focus actions, global keys.
fn draw_bar(frame: &mut Frame, bar: &Bar, area: Rect) {
    let width = area.width as usize;
    let rule = width.saturating_sub(4 + bar.title.chars().count());
    let title = Line::from(vec![
        Span::raw("  "),
        Span::styled(bar.title, canvas::STANDARD),
        Span::styled(format!(" {}", "─".repeat(rule)), canvas::STANDARD),
    ]);
    let mut actions = vec![Span::raw("  ")];
    for (i, a) in bar.actions.iter().enumerate() {
        if i > 0 {
            actions.push(Span::raw(bar.separator));
        }
        let rest = if a.primary {
            canvas::STANDARD.add_modifier(Modifier::BOLD)
        } else {
            canvas::STANDARD
        };
        actions.extend(keyed_spans(&a.text, canvas::FOCUS, rest));
    }
    let mut lines = vec![title, Line::default(), Line::from(actions)];
    if let Some(globals) = &bar.globals {
        // If the keys don't fit, end with `[?]more` (§6.4).
        let room = width.saturating_sub(10);
        let mut shown: Vec<&str> = Vec::new();
        let mut used = 0;
        for (i, key) in globals.iter().enumerate() {
            let more = if i + 1 < globals.len() { 8 } else { 0 };
            if used + key.chars().count() + more > room {
                shown.push("[?]more");
                break;
            }
            used += key.chars().count() + 1;
            shown.push(key);
        }
        let mut line = vec![Span::raw("  "), Span::styled("global  ", canvas::DIM)];
        line.extend(keyed_spans(
            &shown.join(" "),
            canvas::STANDARD,
            canvas::STANDARD,
        ));
        lines.push(Line::from(line));
    }
    frame.render_widget(Paragraph::new(lines).style(canvas::STANDARD), area);
}

/// Below the minimum size the flow isn't drawn (§4.6).
fn draw_too_small(frame: &mut Frame, app: &App, area: Rect) {
    let files = app.dir.join("*.toml");
    let lines = [
        format!(
            "The terminal is too small for the editor: {}×{}, needs at least {}×{}.",
            area.width,
            area.height,
            canvas::MIN_WIDTH,
            canvas::MIN_HEIGHT
        ),
        String::new(),
        "Make the window bigger, or edit the node files by hand:".into(),
        format!("  {}", files.display()),
        String::new(),
        "q quit".into(),
    ];
    let text: Vec<Line> = lines.into_iter().map(Line::from).collect();
    let inner = Rect {
        x: area.x + 2,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(1),
    };
    frame.render_widget(
        Paragraph::new(text)
            .style(canvas::STANDARD)
            .wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

/// A two-cell block of `c` (blended over the background by its alpha).
fn swatch(c: Rgba<u8>) -> Span<'static> {
    Span::styled("██", Style::new().fg(rgb(c)))
}

/// Terminal color for `c`, with transparency shown by blending over dark grey.
fn rgb(c: Rgba<u8>) -> Color {
    let a = u16::from(c[3]);
    let mix = |v: u8| ((u16::from(v) * a + 40 * (255 - a)) / 255) as u8;
    Color::Rgb(mix(c[0]), mix(c[1]), mix(c[2]))
}

fn color_value(v: &toml::Value) -> Option<Rgba<u8>> {
    v.as_str().and_then(|s| parse_color(s).ok())
}

/// Below this width the three panes are stacked (inputs, node, used by) instead of
/// side by side, so each gets the full width.
const STACK_BELOW_WIDTH: u16 = 100;

fn lineage_areas(main: Rect, node_rows: usize) -> [Rect; 3] {
    if main.width >= STACK_BELOW_WIDTH {
        return Layout::horizontal([
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ])
        .areas(main);
    }
    // The node panel gets the height it needs (up to half); the trees share the rest.
    let node_height = (node_rows as u16 + 2).clamp(3, (main.height / 2).max(3));
    Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(node_height),
        Constraint::Fill(1),
    ])
    .areas(main)
}

fn draw_title(frame: &mut Frame, app: &App, area: Rect) {
    let invalid = app.project.errors.len();
    let nodes = app.project.nodes.len();
    let s = if nodes == 1 { "" } else { "s" };
    let mut spans = vec![
        Span::styled(" conduit edit ", Style::new().bold().fg(ACCENT)),
        Span::raw(app.dir.display().to_string()),
        Span::styled(format!("  ·  {nodes} node{s}"), DIM),
    ];
    // The flow view counts errors in its status line (§5.4).
    if invalid > 0 && !app.flow_view {
        spans.push(Span::styled(format!("  ·  {invalid} problem(s)"), ERROR));
    }
    frame.render_widget(Line::from(spans), area);
}

fn pane_block(app: &App, pane: Pane, title: Line<'static>) -> Block<'static> {
    let active = app.pane == pane && app.mode == Mode::Normal;
    Block::bordered()
        .border_type(if active {
            BorderType::Thick
        } else {
            BorderType::Plain
        })
        .border_style(if active { Style::new().fg(ACCENT) } else { DIM })
        .title(title)
}

fn highlight(app: &App, pane: Pane) -> Style {
    if app.pane == pane {
        Style::new().bg(SELECTED_BG).add_modifier(Modifier::BOLD)
    } else {
        Style::new().add_modifier(Modifier::UNDERLINED)
    }
}

fn kind_of(app: &App, id: &str) -> String {
    app.project
        .get(id)
        .and_then(|n| n.kind.clone())
        .unwrap_or_default()
}

fn has_errors(app: &App, id: &str) -> bool {
    !app.project.errors_for(id).is_empty()
}

fn draw_tree(frame: &mut Frame, app: &mut App, pane: Pane, area: Rect) {
    let (tree, title) = match pane {
        Pane::Up => (&app.up, "← inputs"),
        Pane::Down => (&app.down, "used by →"),
        Pane::Node => unreachable!(),
    };
    let visible = app.visible(pane);
    let items: Vec<ListItem> = visible
        .iter()
        .map(|&i| tree_line(app, pane, tree, i))
        .map(ListItem::new)
        .collect();
    let empty = items.is_empty();
    let block = pane_block(app, pane, Line::from(format!(" {title} ({}) ", tree.len())));
    let list = List::new(items)
        .block(block)
        .highlight_style(highlight(app, pane));
    if empty {
        let text = if pane == Pane::Up {
            "no inputs"
        } else {
            "nothing uses this node"
        };
        frame.render_widget(
            Paragraph::new(Span::styled(text, DIM)).block(pane_block(
                app,
                pane,
                Line::from(format!(" {title} ")),
            )),
            area,
        );
        return;
    }
    let state: &mut ListState = &mut app.lists[pane as usize];
    frame.render_stateful_widget(list, area, state);
}

fn tree_line(app: &App, pane: Pane, tree: &[TreeItem], i: usize) -> Line<'static> {
    let item = &tree[i];
    let has_children = tree.get(i + 1).is_some_and(|next| next.depth > item.depth);
    let marker = match (has_children, app.is_collapsed(pane, &item.node)) {
        (true, true) => "▸ ",
        (true, false) => "▾ ",
        _ => "  ",
    };
    let mut spans = vec![Span::raw(format!(
        "{}{marker}",
        "  ".repeat(item.depth - 1)
    ))];
    if item.missing {
        spans.push(Span::styled(format!("? {} (missing)", item.node), ERROR));
        return Line::from(spans);
    }
    let name_style = if has_errors(app, &item.node) {
        ERROR
    } else {
        Style::new()
    };
    match pane {
        // `port ← node`: this node feeds its parent's `port`.
        Pane::Up => {
            spans.push(Span::styled(format!("{} ← ", item.port), DIM));
            let source = Reference {
                node: item.node.clone(),
                port: item.output.clone(),
            };
            spans.push(Span::styled(source.short(), name_style));
        }
        // `node.port`: this node's `port` takes its parent.
        _ => {
            spans.push(Span::styled(item.node.clone(), name_style));
            spans.push(Span::styled(format!(".{}", item.port), DIM));
        }
    }
    if item.repeat {
        spans.push(Span::styled(" ↺", DIM));
    } else {
        spans.push(Span::styled(format!("  {}", kind_of(app, &item.node)), DIM));
    }
    Line::from(spans)
}

fn draw_node(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(node) = app.focused().cloned() else {
        let block = pane_block(app, Pane::Node, Line::from(" no nodes "));
        let hint = "this folder has no node files yet";
        frame.render_widget(Paragraph::new(Span::styled(hint, DIM)).block(block), area);
        return;
    };
    let broken = has_errors(app, &node.id);
    let title = Line::from(vec![
        Span::raw(" "),
        Span::styled(
            if broken { "✗ " } else { "● " },
            if broken {
                ERROR
            } else {
                Style::new().fg(ACCENT)
            },
        ),
        Span::styled(node.id.clone(), Style::new().bold()),
        Span::styled(
            format!("  {} ", node.kind.as_deref().unwrap_or("(unreadable)")),
            DIM,
        ),
    ]);
    let items: Vec<ListItem> = app
        .rows
        .iter()
        .map(|row| ListItem::new(row_line(app, &node, row)))
        .collect();
    let list = List::new(items)
        .block(pane_block(app, Pane::Node, title))
        .highlight_style(highlight(app, Pane::Node));
    frame.render_stateful_widget(list, area, &mut app.lists[Pane::Node as usize]);
}

fn row_line(app: &App, node: &ProjectNode, row: &Row) -> Line<'static> {
    let label = |tag: &str, name: String| {
        vec![
            Span::styled(format!("{tag:<4}"), DIM),
            Span::raw(format!("{name:<10} ")),
        ]
    };
    match row {
        Row::Input {
            port,
            index,
            source,
        } => {
            let name = match index {
                Some(i) => format!("{port}[{i}]"),
                None => port.clone(),
            };
            let mut spans = label("in", name);
            match source {
                Some(s) => {
                    let style = if app.project.nodes.contains_key(&s.node) {
                        Style::new()
                    } else {
                        ERROR
                    };
                    spans.push(Span::styled("← ", DIM));
                    spans.push(Span::styled(s.short(), style));
                }
                None => {
                    let optional = node
                        .spec
                        .as_ref()
                        .and_then(|s| s.input(port))
                        .is_some_and(|p| p.optional);
                    spans.push(if optional {
                        Span::styled("(optional, not connected)", DIM)
                    } else {
                        Span::styled("not connected", ERROR)
                    });
                }
            }
            Line::from(spans)
        }
        Row::Param {
            name,
            link: Some(link),
        } => {
            let mut spans = label("par", name.clone());
            let style = if app.project.nodes.contains_key(&link.node) {
                Style::new().fg(Color::Cyan)
            } else {
                ERROR
            };
            spans.push(Span::styled("← ", DIM));
            spans.push(Span::styled(link.short(), style));
            match app.linked_value(link) {
                Some(Shown::Number(n)) => spans.push(Span::styled(format!("  = {n}"), DIM)),
                Some(Shown::Color(c)) => {
                    spans.push(Span::raw("  "));
                    spans.push(swatch(c));
                    spans.push(Span::styled(format!(" {}", to_hex(c)), DIM));
                }
                None => {}
            }
            Line::from(spans)
        }
        Row::Param { name, link: None } => {
            let mut spans = label("par", name.clone());
            let spec: Option<&ParamSpec> = node.spec.as_ref().and_then(|s| s.param(name));
            match (node.params.get(name), spec) {
                (Some(value), Some(_)) => {
                    spans.push(Span::raw(format!("= {value}")));
                    if let Some(c) = color_value(value) {
                        spans.push(Span::raw(" "));
                        spans.push(swatch(c));
                    }
                }
                (Some(value), None) => spans.push(Span::styled(
                    format!("= {value}  (unknown param)"),
                    Style::new().fg(Color::Yellow),
                )),
                (None, Some(p)) if p.required => spans.push(Span::styled("missing", ERROR)),
                (None, Some(p)) => spans.push(Span::styled(
                    match p.default {
                        Some(d) => format!("= {d}  (default)"),
                        None => "(unset)".to_string(),
                    },
                    DIM,
                )),
                (None, None) => {}
            }
            Line::from(spans)
        }
        Row::Entry { param, key } => {
            let mut spans = label("col", key.clone());
            let value = node.params.get(param).and_then(|t| t.get(key));
            match value.and_then(color_value) {
                Some(c) => {
                    spans.push(swatch(c));
                    spans.push(Span::raw(format!(" {}", to_hex(c))));
                }
                None => spans.push(Span::styled(
                    format!(
                        "{}  (not a color)",
                        value.map(|v| v.to_string()).unwrap_or_default()
                    ),
                    ERROR,
                )),
            }
            spans.extend(users_spans(app, &node.id, key, true));
            Line::from(spans)
        }
        Row::Output { port } => {
            let mut spans = label("out", port.clone());
            spans.extend(users_spans(app, &node.id, port, false));
            Line::from(spans)
        }
        Row::Error(message) => Line::from(Span::styled(format!("!   {message}"), ERROR)),
    }
}

/// `→ a.color, b.color` for the users of `node.port` (`→ (unused)` unless `quiet`).
fn users_spans(app: &App, node: &str, port: &str, quiet: bool) -> Vec<Span<'static>> {
    let users: Vec<_> = (app.project.consumers(node).iter())
        .filter(|c| c.output == port)
        .collect();
    match (users.is_empty(), quiet) {
        (true, true) => vec![],
        (true, false) => vec![Span::styled("→ (unused)", DIM)],
        (false, true) => vec![Span::styled("  → ", DIM), Span::raw(uses(users))],
        (false, false) => vec![Span::styled("→ ", DIM), Span::raw(uses(users))],
    }
}

fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    // The probe label gets its own space on the right so it never overwrites the status.
    let probe = app.probe.as_ref().map(|probe| {
        let follow = if app.probe_follow { " (follow)" } else { "" };
        format!("  probe: {probe}{follow} ")
    });
    let probe_width = probe.as_ref().map_or(0, |p| p.chars().count() as u16);
    let [area, probe_area] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(probe_width)]).areas(area);
    if let Some(text) = probe {
        frame.render_widget(
            Line::from(Span::styled(text, Style::new().fg(ACCENT))),
            probe_area,
        );
    }

    let mut spans = vec![Span::raw(" ")];
    match &app.status {
        None => spans.push(Span::styled("starting…", DIM)),
        Some(Event::Rendered { stats, elapsed, .. }) => spans.push(Span::styled(
            format!("● {}", run::describe_render(stats, *elapsed)),
            Style::new().fg(Color::Green),
        )),
        Some(Event::Failed(message)) => {
            let mut lines = message.lines();
            let first = lines.next().unwrap_or_default();
            let more = lines.count();
            let suffix = if more > 0 {
                format!("  (+{more} more)")
            } else {
                String::new()
            };
            spans.push(Span::styled(format!("✗ {first}{suffix}"), ERROR));
        }
        Some(Event::Warning(message)) => spans.push(Span::styled(
            format!("! {}", message.lines().next().unwrap_or_default()),
            Style::new().fg(Color::Yellow),
        )),
        Some(Event::FilesChanged) => {}
    }
    if let Some(id) = &app.carrying {
        spans.push(Span::styled(
            format!("   ⇄ moving {id}"),
            Style::new().fg(ACCENT),
        ));
    }
    if let Some(message) = &app.message {
        spans.push(Span::styled(
            format!("   {message}"),
            Style::new().fg(Color::Yellow),
        ));
    }
    frame.render_widget(Line::from(spans), area);
}

fn draw_keys(frame: &mut Frame, app: &App, area: Rect) {
    let hints = match (&app.mode, app.pane) {
        (
            Mode::Pick {
                purpose: Pick::Font { .. } | Pick::File { .. },
                ..
            },
            _,
        ) => "type to filter   ↑/↓ preview in place   ⏎ keep   esc put back",
        (Mode::Pick { .. }, _) => "type to filter   ↑/↓ select   ⏎ choose   esc cancel",
        (Mode::Prompt { selected: true, .. }, _) => {
            "type to replace   → keep and edit   ⏎ save   esc cancel"
        }
        (Mode::Prompt { .. }, _) => "⏎ save   esc cancel   ctrl-u clear",
        (Mode::Confirm { .. }, _) => "y confirm   any other key cancels",
        (Mode::Help, _) => "any key to close",
        (Mode::Color(_), _) => {
            "j/k channel  +/- 1  >/< 10  }/{ 50  tab HSL/RGB  # hex  ⏎ keep  esc revert"
        }
        _ if app.carrying.is_some() => {
            "go to a connection (j/k, H/L, h/l)   m/⏎ put it there   esc cancel"
        }
        (_, Pane::Node) => {
            "h/l follow   j/k move   ⏎ edit   c/d connect   i insert   a add   r/D rename/del   u undo   / find   p probe   ? help"
        }
        (_, Pane::Up) => {
            "h/⏎ focus   l back   j/k move   i insert   m move   X dissolve   space fold   ? help"
        }
        (_, Pane::Down) => {
            "l/⏎ focus   h back   j/k move   i insert   m move   X dissolve   space fold   ? help"
        }
    };
    frame.render_widget(Line::from(Span::styled(format!(" {hints}"), DIM)), area);
}

fn popup(frame: &mut Frame, width: u16, height: u16) -> Rect {
    let area = frame
        .area()
        .centered(Constraint::Length(width), Constraint::Length(height));
    frame.render_widget(Clear, area);
    area
}

fn overlay_block(title: String) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Thick)
        .border_style(Style::new().fg(ACCENT))
        .title(title)
}

fn draw_pick(frame: &mut Frame, app: &App) {
    let Mode::Pick {
        purpose,
        query,
        selected,
    } = &app.mode
    else {
        return;
    };
    let matches = app.pick_matches();
    let total = app.pick_items(purpose).len();
    let focus = app.focus.as_deref().unwrap_or_default();
    let area = popup(frame, 64, 20);
    let block = overlay_block(format!(
        " {} ({} of {total}) ",
        purpose.title(focus),
        matches.len()
    ));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let [input, list_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
    frame.render_widget(
        Line::from(vec![
            Span::styled("/ ", Style::new().fg(ACCENT)),
            Span::raw(query.clone()),
            Span::styled("▏", Style::new().fg(ACCENT)),
        ]),
        input,
    );
    // Labels are "name  details"; dim the details.
    let items: Vec<ListItem> = matches
        .iter()
        .map(|(_, label)| {
            let (name, rest) = label.split_once("  ").unwrap_or((label, ""));
            ListItem::new(Line::from(vec![
                Span::raw(name.to_string()),
                Span::styled(format!("  {rest}"), DIM),
            ]))
        })
        .collect();
    let mut state = ListState::default().with_selected((!matches.is_empty()).then_some(*selected));
    frame.render_stateful_widget(
        List::new(items).highlight_style(Style::new().bg(SELECTED_BG).add_modifier(Modifier::BOLD)),
        list_area,
        &mut state,
    );
}

fn draw_prompt(frame: &mut Frame, app: &App) {
    let Mode::Prompt {
        purpose,
        input,
        selected,
        error,
    } = &app.mode
    else {
        return;
    };
    let focus = app.focus.as_deref().unwrap_or_default();
    let area = popup(frame, 64, if error.is_some() { 4 } else { 3 });
    // A highlighted value is replaced by whatever is typed next.
    let text = if *selected {
        Span::styled(input.clone(), Style::new().fg(Color::Black).bg(ACCENT))
    } else {
        Span::raw(input.clone())
    };
    let mut lines = vec![Line::from(vec![
        text,
        Span::styled("▏", Style::new().fg(ACCENT)),
    ])];
    if let Some(error) = error {
        lines.push(Line::from(Span::styled(error.clone(), ERROR)));
    }
    let block = overlay_block(format!(" {} ", purpose.label(focus)));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_confirm(frame: &mut Frame, app: &App) {
    let Mode::Confirm { question, .. } = &app.mode else {
        return;
    };
    let area = popup(frame, 64, 5);
    let lines = vec![
        Line::from(question.clone()),
        Line::from(""),
        Line::from(Span::styled("y to confirm, any other key to cancel", DIM)),
    ];
    let block = overlay_block(" confirm ".into());
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .block(block),
        area,
    );
}

const HELP: &[(&str, &str)] = &[
    (
        "h / l",
        "follow the connection under the cursor upstream / downstream",
    ),
    ("j / k", "move the cursor (g / G: top / bottom)"),
    ("H / L", "move into the inputs / used-by tree"),
    (
        "⏎",
        "edit what's under the cursor (colors: color editor); tree: focus it",
    ),
    ("space", "in a tree: fold / unfold"),
    ("tab", "cycle panes (esc: back to the node)"),
    ("v", "switch between the flow view and the three panes"),
    (
        "[ / ]",
        "back / forward through focused nodes (also ctrl-o, backspace)",
    ),
    ("/", "find a node by name or type"),
    (
        "c / d",
        "connect / disconnect the input, or link / unlink a number",
    ),
    (
        "i",
        "insert a node into a connection or tree row; number: feed from int",
    ),
    (
        "⏎ on a font / file",
        "pick a font or project file (e.g. an image); the preview shows each as you move",
    ),
    (
        "K / J",
        "move a list connection earlier / later (composite: later is on top)",
    ),
    ("+ / -", "nudge a number by 1 (< / > by 10, { / } by 50)"),
    ("a", "add a new node"),
    (
        "o / x",
        "add / remove a palette color (r on a color renames it)",
    ),
    (
        "I",
        "import colors into a palette (Coolors link or hex codes)",
    ),
    (
        "C",
        "open the color under the cursor in the GUI color picker",
    ),
    (
        "r / D",
        "rename / delete the node (in a tree: the highlighted one)",
    ),
    (
        "X",
        "dissolve: delete the node, connecting what fed it to its users",
    ),
    (
        "m",
        "move the node: pick it up, then m / ⏎ on a connection to put it",
    ),
    ("E", "open the focused node's file in $EDITOR"),
    ("u", "undo the last edit"),
    (
        "p",
        "probe: show the output under the cursor in a probe window",
    ),
    ("P", "probe follows focus on / off"),
    ("q", "quit"),
];

/// Width of a channel's gradient bar in the color editor.
const BAR: usize = 32;

fn draw_color(frame: &mut Frame, edit: &ColorEdit) {
    let area = popup(frame, 58, 11);
    let color = edit.color();
    let mut lines = vec![
        Line::from(vec![
            Span::raw(" "),
            Span::styled("█".repeat(16), Style::new().fg(rgb(color))),
            Span::raw(format!("  {}", to_hex(color))),
        ]),
        Line::from(vec![
            Span::raw(" "),
            Span::styled("█".repeat(16), Style::new().fg(rgb(color))),
            Span::styled(format!("  was {}", to_hex(edit.original)), DIM),
            Span::raw(" "),
            swatch(edit.original),
        ]),
        Line::raw(""),
    ];
    for (i, ch) in edit.channels().into_iter().enumerate() {
        let selected = i == edit.channel;
        let marker = if selected { "▸ " } else { "  " };
        let name_style = if selected {
            Style::new().fg(ACCENT).bold()
        } else {
            Style::new()
        };
        let mut spans = vec![
            Span::styled(format!(" {marker}{} ", ch.letter()), name_style),
            Span::raw(format!("{:>4}  ", edit.model.value_text(ch))),
        ];
        // The channel's range as a gradient, with the current position marked.
        let at = (edit.model.position(ch) * (BAR - 1) as f64).round() as usize;
        for cell in 0..BAR {
            let c = rgb(edit.model.with(ch, cell as f64 / (BAR - 1) as f64));
            spans.push(if cell == at {
                Span::styled("┃", Style::new().fg(Color::White).bg(c).bold())
            } else {
                Span::styled("█", Style::new().fg(c))
            });
        }
        lines.push(Line::from(spans));
    }
    if let Some(input) = &edit.hex {
        lines.push(Line::from(vec![
            Span::raw(format!("   {input}")),
            Span::styled("▏", Style::new().fg(ACCENT)),
            Span::styled("  ⏎ apply  esc cancel", DIM),
        ]));
    }
    let mode = if edit.rgb { "RGB" } else { "HSL" };
    let block = overlay_block(format!(" color  {}  ({mode}) ", edit.target.label()));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// Keys of the flow view, grouped as in the spec's §6.
const FLOW_HELP: &[(&str, &str)] = &[
    (
        "h l j k",
        "walk the flow: node, conduit, node; j / k into a node's rows",
    ),
    ("esc", "back to the node; in the edit view, close it"),
    ("g / G", "first / last node of the flow"),
    ("f", "on a ▣ end: bring its conduit into the flow"),
    ("[ / ]", "back / forward through flow changes"),
    ("⏎", "the focus's primary action (bold in the bar)"),
    (
        "e",
        "edit: a node opens the edit view; a param or name its prompt",
    ),
    ("c", "connect: pick a source; on an output, a new user"),
    ("i", "insert a new node into a conduit, or feeding an input"),
    (
        "d",
        "dissolve a node; disconnect; reset a param to its default",
    ),
    ("K / J", "move a list entry earlier / later"),
    ("H / L", "slide a node upstream / downstream"),
    ("m", "pick up a node; m or ⏎ on a conduit drops it there"),
    (
        "+-><}{",
        "edit view: nudge a number by 1 / 10 / 50; C colour picker",
    ),
    (
        "/  a",
        "find a node (the flow is rebuilt around it) / add a node",
    ),
    ("r / D", "rename / delete the node"),
    ("u", "undo"),
    ("p / P", "probe the focus / follow the focus"),
    (
        "E  v",
        "open the node in $EDITOR / switch to the three panes",
    ),
    ("q", "quit"),
];

fn draw_help(frame: &mut Frame, help: &[(&str, &str)]) {
    let area = popup(frame, 78, help.len() as u16 + 4);
    let lines: Vec<Line> = help
        .iter()
        .map(|(key, what)| {
            Line::from(vec![
                Span::styled(format!("  {key:<8}"), Style::new().fg(ACCENT).bold()),
                Span::raw(*what),
            ])
        })
        .collect();
    let block = Block::bordered()
        .border_type(BorderType::Thick)
        .border_style(Style::new().fg(ACCENT))
        .title(" keys ");
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use conduit_core::{EvalStats, inspect};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent};

    use super::super::app::tests as app_tests;
    use super::*;

    fn render(app: &mut App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn example_dir(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples")
            .join(name)
    }

    /// An app on the project in `dir`, which it calls `shown`, showing the three panes.
    fn app_at(dir: &Path, shown: &Path) -> App {
        let mut app = flow_app_at(dir, shown);
        app.flow_view = false;
        app
    }

    /// As [`app_at`], showing the flow view.
    fn flow_app_at(dir: &Path, shown: &Path) -> App {
        let registry = Arc::new(conduit_nodes::registry());
        let project = inspect(dir, &registry).unwrap();
        App::new(shown.to_path_buf(), project, registry)
    }

    #[test]
    fn flow_view_draws_the_flow_around_the_focus() {
        let mut app = flow_app_at(&example_dir("effects"), Path::new("examples/effects"));
        let screen = render(&mut app, 80, 24);
        let lines: Vec<&str> = screen.lines().collect();
        assert!(lines[0].contains("conduit edit examples/effects"));
        assert_eq!(
            lines[3].trim_end(),
            "    ╭─ rect ───╮      ╭─ composite ─╮      ╭─ preview ─╮"
        );
        assert_eq!(
            lines[6].trim_end(),
            "    │      out─┼──────┼─base        │  ╭───┼─image     │"
        );
        // Black background everywhere, even off the canvas.
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(79, 23)].bg, Color::Black);
    }

    /// The screen with trailing spaces trimmed from each line, and trailing blank lines.
    fn trimmed(screen: &str) -> String {
        let lines: Vec<&str> = screen.lines().map(str::trim_end).collect();
        lines.join("\n").trim_end().to_string()
    }

    #[test]
    fn flow_view_matches_story_two() {
        // docs/stories/02-exploring.md, steps 1 and 2.
        let mut app = flow_app_at(&example_dir("effects"), Path::new("examples/effects"));
        app.on_runner_event(rendered(13, 13, 41, Default::default()));
        let screen = trimmed(&render(&mut app, 80, 24));
        assert_eq!(screen, STORY_2_STEP_1.trim_end());

        app.handle_key(KeyEvent::from(KeyCode::Char('h')));
        let screen = trimmed(&render(&mut app, 80, 24));
        assert!(
            screen.ends_with(STORY_2_STEP_2_BOTTOM.trim_end()),
            "{screen}"
        );
    }

    const STORY_2_STEP_1: &str = " conduit edit examples/effects  ·  13 nodes


    ╭─ rect ───╮      ╭─ composite ─╮      ╭─ preview ─╮
    │ bg       │      │ comp        │      │ preview   │
    │──────────│      │─────────────│      │───────────│
    │      out─┼──────┼─base        │  ╭───┼─image     │
    ╰──────────╯    ▣─┼─layers[0]   │  │   ╰───────────╯
                    ▣─┼─layers[1]   │  │
                    ▣─┼─layers[2]   │  │
                    ·─┼─layers      │  │
                      │─────────────│  │
                      │         out─┼──╯
                      ╰─────────────╯



 rendered 13 nodes in 41 ms

  Node Actions ────────────────────────────────────────────────────────────────

  (e)dit (d)issolve
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit
";

    const STORY_2_STEP_2_BOTTOM: &str = " comp.out → preview.image

  Conduit Actions ─────────────────────────────────────────────────────────────

  (c)onnect (i)nsert (d)isconnect
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit
";

    /// Code block `n` of a story, e.g. `docs/stories/01-first-image.md`.
    fn story_screen(story: &str, n: usize) -> String {
        let block = story.split("```").nth(2 * n + 1).expect("no such block");
        trimmed(block.trim_start_matches('\n'))
    }

    fn keys(app: &mut App, keys: &str) {
        for c in keys.chars() {
            let code = match c {
                '\n' => KeyCode::Enter,
                '\x1b' => KeyCode::Esc,
                c => KeyCode::Char(c),
            };
            app.handle_key(KeyEvent::from(code));
        }
    }

    #[test]
    fn edit_view_matches_story_one() {
        // docs/stories/01-first-image.md, step 2 (the colour editor aside).
        let story = include_str!("../../../../docs/stories/01-first-image.md");
        let dir = tempfile::tempdir().unwrap();
        // The story has `color` default to white; the real `rect` requires it.
        let file = "type = \"rect\"\n[params]\ncolor = \"white\"\n";
        std::fs::write(dir.path().join("blue_rect.toml"), file).unwrap();
        let white = |screen: String| {
            screen.replace("color  = ██ white (default)", "color  = ██ white          ")
        };
        let mut app = flow_app_at(dir.path(), dir.path());
        let path = dir.path().display().to_string();
        let screen = |app: &mut App| trimmed(&render(app, 80, 24)).replace(&path, "story");

        keys(&mut app, "e");
        assert_eq!(screen(&mut app), white(story_screen(story, 5)));
        keys(&mut app, "j");
        assert_eq!(screen(&mut app), white(story_screen(story, 6)));

        keys(&mut app, "je800\nje600\n");
        // The story's colour is blue by now; ours is still white.
        let expected = story_screen(story, 9).replace("██ blue ", "██ white");
        assert_eq!(screen(&mut app), expected);

        keys(&mut app, "\x1b");
        assert_eq!(app.edit_view, None);
        // The story's status line also gives the image size, which needs a render.
        let expected = story_screen(story, 10).replace("  ·  800×600", "");
        assert_eq!(screen(&mut app), expected);
    }

    #[test]
    fn flow_view_too_small_only_quits() {
        let mut app = flow_app_at(&example_dir("basic"), Path::new("examples/basic"));
        let screen = render(&mut app, 62, 18);
        // The message wraps at this width.
        assert!(screen.contains("too small for the editor: 62×18"));
        assert!(screen.contains("80×24."));
        assert!(screen.contains("examples/basic/*.toml"));
        let focus = app.focus.clone();
        app.handle_key(KeyEvent::from(KeyCode::Char('h')));
        assert_eq!(app.focus, focus);
        assert_eq!(
            app.handle_key(KeyEvent::from(KeyCode::Char('q'))),
            Some(super::super::app::Action::Quit)
        );
        // Growing back draws the flow again.
        let screen = render(&mut app, 80, 24);
        assert!(screen.contains("╭─ preview"));
        render(&mut app, 1, 1);
    }

    #[test]
    fn flow_view_empty_project_hint() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = flow_app_at(dir.path(), Path::new("story"));
        let screen = render(&mut app, 80, 24);
        assert!(screen.contains("This project has no nodes yet."));
        assert!(screen.contains("Press a to add one."));
    }

    fn example() -> App {
        app_at(&example_dir("basic"), Path::new("examples/basic"))
    }

    fn palette_example() -> App {
        let dir = example_dir("palette");
        app_at(&dir, &dir)
    }

    fn rendered(
        nodes: usize,
        evaluated: usize,
        ms: u64,
        shown: HashMap<Reference, Shown>,
    ) -> Event {
        Event::Rendered {
            stats: EvalStats { nodes, evaluated },
            shown,
            elapsed: Duration::from_millis(ms),
        }
    }

    #[test]
    fn lineage_view() {
        let mut app = example();
        app.handle_key(KeyEvent::from(KeyCode::Char('h'))); // focus comp
        app.on_runner_event(rendered(5, 5, 18, Default::default()));
        let screen = render(&mut app, 120, 16);
        for expected in [
            "conduit edit examples/basic",
            "● comp  composite",
            "base ← white_bg",
            "layers ← placed",
            "▾ ",
            "layer ← black_sq",
            "in  base       ← white_bg",
            "in  layers[0]  ← placed",
            "out out        → preview.image",
            "preview.image  preview",
            "● rendered 5 nodes in 18 ms",
        ] {
            assert!(
                screen.contains(expected),
                "missing {expected:?} in:\n{screen}"
            );
        }
    }

    #[test]
    fn linked_params_show_source_and_value() {
        let dir = example_dir("linked");
        let mut app = app_at(&dir, &dir);
        go(&mut app, "/square");
        app.on_runner_event(rendered(
            7,
            1,
            3,
            [(Reference::parse("size").unwrap(), Shown::Number(120.0))].into(),
        ));
        let screen = render(&mut app, 120, 16);
        for expected in [
            "par color      = \"#e63946\"",
            "par width      ← size  = 120",
            "par height     ← size  = 120",
            "width ← size  int",
            "● rendered 7 nodes (6 unchanged) in 3 ms",
        ] {
            assert!(
                screen.contains(expected),
                "missing {expected:?} in:\n{screen}"
            );
        }
        assert!(!screen.contains("in  width"), "{screen}");
    }

    #[test]
    fn find_and_help_overlays() {
        let mut app = example();
        for c in "/bl".chars() {
            app.handle_key(KeyEvent::from(KeyCode::Char(c)));
        }
        let screen = render(&mut app, 120, 30);
        assert!(screen.contains("/ bl"), "{screen}");
        assert!(screen.contains("black_sq  rect"), "{screen}");

        app.handle_key(KeyEvent::from(KeyCode::Esc));
        app.handle_key(KeyEvent::from(KeyCode::Char('?')));
        let screen = render(&mut app, 120, 30);
        assert!(
            screen.contains("follow the connection under the cursor"),
            "{screen}"
        );
    }

    #[test]
    fn font_picker_overlay() {
        let (_tmp, mut app) = app_tests::temp_project("text");
        app_tests::open_font_picker_on(&mut app, "title");
        let screen = render(&mut app, 120, 30);
        assert!(screen.contains("font for title.font (6 of 6)"), "{screen}");
        assert!(screen.contains("Beta Mono  mono"), "{screen}");
        assert!(screen.contains("↑/↓ preview in place"), "{screen}");
    }

    #[test]
    fn errors_are_visible() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.toml"), "type = \"transform\"\n").unwrap();
        let mut app = app_at(tmp.path(), tmp.path());
        app.on_runner_event(Event::Failed(
            "a.toml: required input `image` is not connected".into(),
        ));
        let screen = render(&mut app, 120, 12);
        assert!(screen.contains("✗ a  transform"), "{screen}");
        assert!(screen.contains("not connected"), "{screen}");
        assert!(screen.contains("✗ a.toml: required input"), "{screen}");
        assert!(screen.contains("1 problem(s)"), "{screen}");
    }

    #[test]
    fn narrow_terminal_stacks_panes() {
        let mut app = example();
        app.handle_key(KeyEvent::from(KeyCode::Char('h'))); // focus comp
        app.handle_key(KeyEvent::from(KeyCode::Char('p'))); // probe comp.out
        app.on_runner_event(rendered(5, 5, 18, Default::default()));
        let screen = render(&mut app, 60, 30);
        let line_of = |text: &str| screen.lines().position(|l| l.contains(text));
        // Full text fits, and the panes are stacked top to bottom.
        let inputs = line_of("base ← white_bg  rect").expect(&screen);
        let node = line_of("in  layers[0]  ← placed").expect(&screen);
        let users = line_of("preview.image  preview").expect(&screen);
        assert!(inputs < node && node < users, "{screen}");
        // Status and probe label share the line without overlapping.
        let status = screen
            .lines()
            .find(|l| l.contains("rendered"))
            .expect(&screen);
        assert!(status.contains("● rendered 5 nodes in 18 ms"), "{status}");
        assert!(status.contains("probe: comp.out"), "{status}");
    }

    #[test]
    fn prompt_and_confirm_overlays() {
        let mut app = example();
        app.handle_key(KeyEvent::from(KeyCode::Char('h'))); // comp
        app.handle_key(KeyEvent::from(KeyCode::Char('k'))); // layers[0]
        app.handle_key(KeyEvent::from(KeyCode::Char('h'))); // placed, on `out`
        // Up past filter, the anchors, skews, scales and rotate to y.
        for _ in 0..10 {
            app.handle_key(KeyEvent::from(KeyCode::Char('k')));
        }
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        let screen = render(&mut app, 120, 30);
        assert!(
            screen.contains("type to replace"),
            "the value starts highlighted"
        );
        app.handle_key(KeyEvent::from(KeyCode::Right)); // keep it, to edit
        app.handle_key(KeyEvent::from(KeyCode::Char('x')));
        app.handle_key(KeyEvent::from(KeyCode::Enter)); // "300x" is rejected
        let screen = render(&mut app, 120, 30);
        assert!(screen.contains("placed.y (empty = default)"), "{screen}");
        assert!(screen.contains("300x▏"), "{screen}");
        assert!(screen.contains("`300x` is not a number"), "{screen}");

        app.handle_key(KeyEvent::from(KeyCode::Esc));
        app.handle_key(KeyEvent::from(KeyCode::Char('D')));
        let screen = render(&mut app, 120, 30);
        assert!(
            screen.contains("Delete `placed`? This disconnects comp.layers."),
            "{screen}"
        );
        assert!(screen.contains("y to confirm"), "{screen}");
    }

    #[test]
    fn tiny_terminal_does_not_panic() {
        let mut app = example();
        render(&mut app, 10, 4);
        render(&mut app, 1, 1);
    }

    /// Foreground color of the first cell of `text` on screen.
    fn color_at(app: &mut App, text: &str, offset: u16) -> Color {
        let (width, height) = (120, 24);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buffer = terminal.backend().buffer();
        for y in 0..height {
            let line: String = (0..width).map(|x| buffer[(x, y)].symbol()).collect();
            if let Some(i) = line.find(text) {
                let x = line[..i].chars().count() as u16 + offset;
                return buffer[(x, y)].fg;
            }
        }
        panic!("{text:?} not on screen");
    }

    fn go(app: &mut App, keys: &str) {
        for c in keys.chars() {
            app.handle_key(KeyEvent::from(KeyCode::Char(c)));
        }
        app.handle_key(KeyEvent::from(KeyCode::Enter));
    }

    #[test]
    fn palette_colors_have_swatches() {
        let mut app = palette_example();
        go(&mut app, "/brand");
        let screen = render(&mut app, 120, 24);
        for expected in [
            "col primary    ██ #264653  → stripe.color",
            "col teal       ██ #2a9d8f",
            "col accent     ██ #e76f51  → accent_dark.color",
        ] {
            assert!(
                screen.contains(expected),
                "missing {expected:?} in:\n{screen}"
            );
        }
        assert!(!screen.contains("out "), "{screen}");
        assert_eq!(
            color_at(&mut app, "██ #264653", 0),
            Color::Rgb(0x26, 0x46, 0x53)
        );
    }

    #[test]
    fn linked_colors_show_their_value() {
        let mut app = palette_example();
        go(&mut app, "/square");
        app.on_runner_event(rendered(
            9,
            9,
            3,
            [(
                Reference::parse("accent_dark").unwrap(),
                Shown::Color(Rgba([0xcf, 0x40, 0x1d, 255])),
            )]
            .into(),
        ));
        let screen = render(&mut app, 120, 24);
        assert!(
            screen.contains("par color      ← accent_dark  ██ #cf401d"),
            "{screen}"
        );
        assert_eq!(
            color_at(&mut app, "██ #cf401d", 0),
            Color::Rgb(0xcf, 0x40, 0x1d)
        );
    }

    #[test]
    fn color_editor_overlay() {
        let mut app = palette_example();
        go(&mut app, "/brand");
        for _ in 0..5 {
            app.handle_key(KeyEvent::from(KeyCode::Char('j'))); // past `source` to accent
        }
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        let screen = render(&mut app, 120, 24);
        for expected in [
            "color  brand.accent  (HSL)",
            "#e76f51",
            "was #e76f51",
            "▸ H  12°",
            "  S  76%",
            "  L  61%",
            "  A 100%",
            "j/k channel",
        ] {
            assert!(
                screen.contains(expected),
                "missing {expected:?} in:\n{screen}"
            );
        }
        app.handle_key(KeyEvent::from(KeyCode::Tab));
        let screen = render(&mut app, 120, 24);
        assert!(
            screen.contains("(RGB)") && screen.contains("▸ R  231"),
            "{screen}"
        );
        app.handle_key(KeyEvent::from(KeyCode::Esc));
    }
}
