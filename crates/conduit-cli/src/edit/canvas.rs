//! Draws a [`Flow`] (spec §3, §4): node boxes left to right, joined by the flow's
//! conduits, with `▣` / `·` / `✕` markers on the other rows. The whole flow is drawn into
//! a [`Picture`] at its full size; [`Viewport`] decides which part of it is on screen.

use conduit_core::{Project, ProjectNode, Rgba, parse_color};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use std::collections::HashSet;

use super::flow::{
    Flow, Focus, InRow, InState, OutRow, input_rows, output_rows, same_input, same_output,
};

/// Box content width limits (§3.2): text longer than `MAX_CONTENT` is cut with `…`.
pub const MIN_CONTENT: usize = 8;
pub const MAX_CONTENT: usize = 24;
/// Columns between boxes: room for a `▣─` on each side and a conduit's vertical run.
pub const GAP: u16 = 6;
/// The first box's top row and left column on the picture.
const TOP: u16 = 2;
const LEFT: u16 = 4;
/// The editor's minimum screen size (§4.6).
pub const MIN_WIDTH: u16 = 80;
pub const MIN_HEIGHT: u16 = 24;

// Palette (§5.1).
pub const BACKGROUND: Color = Color::Black;
pub const STANDARD: Style = Style::new().fg(Color::White).bg(BACKGROUND);
pub const FOCUS: Style = Style::new()
    .fg(Color::Green)
    .bg(BACKGROUND)
    .add_modifier(Modifier::BOLD);
pub const RELATED: Style = Style::new().fg(Color::Magenta).bg(BACKGROUND);
pub const DIM: Style = Style::new().fg(Color::DarkGray).bg(BACKGROUND);
pub const ERROR: Style = Style::new().fg(Color::Red).bg(BACKGROUND);

/// A row's marker outside the box (§4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Marker {
    /// The row's conduit is drawn to the next box.
    Flow,
    /// Connected, but the conduit isn't drawn; `broken` if the node at the other end is.
    OffFlow {
        broken: bool,
    },
    Optional,
    Missing,
    /// An output with no users.
    None,
}

/// Where a node box landed on the picture.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub node: String,
    pub x: u16,
    pub width: u16,
    pub height: u16,
    pub inputs: Vec<(InRow, PlacedRow)>,
    pub outputs: Vec<(OutRow, PlacedRow)>,
}

/// Where a port row landed on the picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlacedRow {
    pub y: u16,
    /// The label's first column and width.
    pub label: (u16, u16),
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Cell {
    ch: char,
    style: Style,
}

const BLANK: Cell = Cell {
    ch: ' ',
    style: STANDARD,
};

/// A flow drawn at full size.
#[derive(Debug, Clone, Default)]
pub struct Picture {
    pub width: u16,
    pub height: u16,
    cells: Vec<Cell>,
    pub boxes: Vec<Placed>,
    /// The cells of each flow conduit, by link index (empty if it couldn't be drawn).
    pub conduits: Vec<Vec<(u16, u16)>>,
}

impl Picture {
    fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            cells: vec![BLANK; width as usize * height as usize],
            boxes: Vec::new(),
            conduits: Vec::new(),
        }
    }

    fn cell_mut(&mut self, x: u16, y: u16) -> Option<&mut Cell> {
        if x < self.width && y < self.height {
            self.cells
                .get_mut(y as usize * self.width as usize + x as usize)
        } else {
            None
        }
    }

    fn put(&mut self, x: u16, y: u16, text: &str, style: Style) {
        for (i, ch) in text.chars().enumerate() {
            let x = x as usize + i;
            if x < self.width as usize && y < self.height {
                self.cells[y as usize * self.width as usize + x] = Cell { ch, style };
            }
        }
    }

    /// Copies the part of the picture at `(dx, dy)` into `area`; the rest is background.
    pub fn blit(&self, buf: &mut Buffer, area: Rect, dx: u16, dy: u16) {
        for sy in 0..area.height {
            for sx in 0..area.width {
                let (px, py) = (sx as usize + dx as usize, sy as usize + dy as usize);
                let cell = if px < self.width as usize && py < self.height as usize {
                    self.cells[py * self.width as usize + px]
                } else {
                    BLANK
                };
                if let Some(c) = buf.cell_mut((area.x + sx, area.y + sy)) {
                    c.set_char(cell.ch).set_style(cell.style);
                }
            }
        }
    }

    /// The picture as plain text, trailing spaces trimmed (for tests).
    #[cfg(test)]
    pub fn text(&self) -> String {
        self.cells
            .chunks(self.width as usize)
            .map(|row| {
                row.iter()
                    .map(|c| c.ch)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[cfg(test)]
    fn style_at(&self, x: u16, y: u16) -> Style {
        self.cells[y as usize * self.width as usize + x as usize].style
    }
}

/// `text` cut to `max` columns, ending in `…` if it was longer.
fn fit(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let mut s: String = text.chars().take(max.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}

fn width(text: &str) -> usize {
    text.chars().count()
}

fn is_broken(project: &Project, id: &str) -> bool {
    project
        .get(id)
        .is_none_or(|n| n.spec.is_none() || !project.errors_for(id).is_empty())
}

/// The colour of a palette entry output, for its swatch (§7.1).
fn swatch(node: &ProjectNode, port: &str) -> Option<Rgba<u8>> {
    let spec = node.spec.as_ref()?;
    spec.outputs.iter().filter(|p| p.entries).find_map(|p| {
        let value = node.params.get(p.name)?.as_table()?.get(port)?;
        parse_color(value.as_str()?).ok()
    })
}

/// A node box's text, measured but not yet placed.
struct BoxText {
    kind: String,
    id: String,
    inputs: Vec<(InRow, String, Marker)>,
    /// Row, label (swatch excluded), marker, swatch colour.
    outputs: Vec<(OutRow, String, Marker, Option<Rgba<u8>>)>,
    /// Columns between the borders.
    inner: usize,
    broken: bool,
}

impl BoxText {
    fn new(project: &Project, flow: &Flow, i: usize) -> Self {
        let id = &flow.nodes[i];
        let node = project.get(id).expect("flow nodes are in the project");
        let link_in = flow.link_in(i);
        let link_out = flow.link_out(i);

        let inputs: Vec<(InRow, String, Marker)> = input_rows(node)
            .into_iter()
            .map(|row| {
                let marker = match &row.state {
                    InState::Connected(source) => {
                        let index = row.index.unwrap_or(0);
                        if link_in.is_some_and(|l| l.port == row.port && l.index == index) {
                            Marker::Flow
                        } else {
                            Marker::OffFlow {
                                broken: is_broken(project, &source.node),
                            }
                        }
                    }
                    InState::Slot | InState::Optional => Marker::Optional,
                    InState::Missing => Marker::Missing,
                };
                let label = row.label();
                (row, label, marker)
            })
            .collect();
        let outputs: Vec<_> = output_rows(project, node)
            .into_iter()
            .map(|row| {
                let marker = match &row.user {
                    None => Marker::None,
                    Some(user) => {
                        if link_out.is_some_and(|l| {
                            l.output == row.port
                                && l.target == user.node
                                && l.port == user.port
                                && l.index == user.index
                        }) {
                            Marker::Flow
                        } else {
                            Marker::OffFlow {
                                broken: is_broken(project, &user.node),
                            }
                        }
                    }
                };
                let swatch = swatch(node, &row.port);
                let label = row.port.clone();
                (row, label, marker, swatch)
            })
            .collect();

        let kind = node.kind.clone().unwrap_or_else(|| "?".into());
        let content = [width(&kind) + 2, width(id)]
            .into_iter()
            .chain(inputs.iter().map(|(_, l, _)| width(l)))
            .chain(
                outputs
                    .iter()
                    .map(|(_, l, _, s)| width(l) + if s.is_some() { 3 } else { 0 }),
            )
            .max()
            .unwrap_or(0)
            .clamp(MIN_CONTENT, MAX_CONTENT);
        Self {
            kind,
            id: id.clone(),
            inputs,
            outputs,
            inner: content + 2,
            broken: is_broken(project, id),
        }
    }

    fn width(&self) -> u16 {
        self.inner as u16 + 2
    }

    fn height(&self) -> u16 {
        let section = |n: usize| if n == 0 { 0 } else { n + 1 };
        (3 + section(self.inputs.len()) + section(self.outputs.len())) as u16
    }
}

/// Draws `flow`, with `focus` in green and what it's related to in magenta (§5.2, §5.3).
pub fn draw(project: &Project, flow: &Flow, focus: Option<&Focus>) -> Picture {
    let boxes: Vec<BoxText> = (0..flow.nodes.len())
        .map(|i| BoxText::new(project, flow, i))
        .collect();
    let Some(first) = boxes.first() else {
        return Picture::default();
    };
    let start = LEFT + if first.inputs.is_empty() { 0 } else { 2 };
    let right = start + boxes.iter().map(|b| b.width() + GAP).sum::<u16>() - GAP + 2;
    let bottom = TOP + boxes.iter().map(BoxText::height).max().unwrap_or(0);
    let mut pic = Picture::new(right, bottom);

    let mut x = start;
    // The previous box's flow output: (picture row, column after its `┼`).
    let mut prev: Option<(u16, u16)> = None;
    for (i, b) in boxes.iter().enumerate() {
        let placed = draw_box(&mut pic, b, x);
        if i > 0 {
            let to_y = placed
                .inputs
                .iter()
                .zip(&b.inputs)
                .find(|(_, (_, _, m))| *m == Marker::Flow)
                .map(|((_, r), _)| r.y);
            let cells = match (prev, to_y) {
                (Some((from_y, from_x)), Some(to_y)) => {
                    draw_conduit(&mut pic, (from_x, from_y), (x, to_y))
                }
                _ => Vec::new(),
            };
            pic.conduits.push(cells);
        }
        prev = placed
            .outputs
            .iter()
            .zip(&b.outputs)
            .find(|(_, (_, _, m, _))| *m == Marker::Flow)
            .map(|((_, r), _)| (r.y, x + b.width()));
        pic.boxes.push(placed);
        x += b.width() + GAP;
    }
    if let Some(focus) = focus {
        highlight(&mut pic, flow, focus);
    }
    pic
}

fn draw_box(pic: &mut Picture, b: &BoxText, x: u16) -> Placed {
    let inner = b.inner;
    let border = if b.broken { ERROR } else { STANDARD };
    let content = inner - 2;
    let kind = fit(&b.kind, content - 2);
    let mut y = TOP;
    pic.put(x, y, "╭─ ", border);
    pic.put(x + 3, y, &kind, border);
    let rest = inner - 3 - width(&kind); // `─ ` + kind + ` ` + rest`─` fills `inner`
    pic.put(
        x + 3 + width(&kind) as u16,
        y,
        &format!(" {}╮", "─".repeat(rest)),
        border,
    );
    y += 1;
    pic.put(x, y, "│", border);
    pic.put(x + 2, y, &fit(&b.id, content), STANDARD);
    pic.put(x + inner as u16 + 1, y, "│", border);
    y += 1;

    let divider = format!("│{}│", "─".repeat(inner));
    let mut inputs = Vec::new();
    if !b.inputs.is_empty() {
        pic.put(x, y, &divider, border);
        y += 1;
        for (row, label, marker) in &b.inputs {
            let dim = *marker == Marker::Optional;
            let label = fit(label, content);
            pic.put(x, y, "┼─", border);
            pic.put(x + 2, y, &label, if dim { DIM } else { STANDARD });
            pic.put(x + inner as u16 + 1, y, "│", border);
            let (mark, style) = match marker {
                Marker::OffFlow { broken: false } => ("▣─", STANDARD),
                Marker::OffFlow { broken: true } => ("▣─", ERROR),
                Marker::Optional => ("·─", DIM),
                Marker::Missing => ("✕─", ERROR),
                Marker::Flow | Marker::None => ("", STANDARD),
            };
            if !mark.is_empty() {
                pic.put(x - 2, y, mark, style);
            }
            let label = (x + 2, width(&label) as u16);
            inputs.push((row.clone(), PlacedRow { y, label }));
            y += 1;
        }
    }
    let mut outputs = Vec::new();
    if !b.outputs.is_empty() {
        pic.put(x, y, &divider, border);
        y += 1;
        for (row, label, marker, colour) in &b.outputs {
            let room = content - if colour.is_some() { 3 } else { 0 };
            let label = fit(label, room);
            let end = x + inner as u16 + 1; // the `┼` column
            pic.put(x, y, "│", border);
            let label_x = if let Some(c) = colour {
                // A palette colour (§7.1): swatches in a column, `██ name───┼`.
                pic.put(x + 2, y, "██", STANDARD.fg(rgb(*c)));
                let dashes = "─".repeat(inner - 4 - width(&label));
                pic.put(x + 5, y, &format!("{label}{dashes}"), STANDARD);
                x + 5
            } else {
                let label_x = end - 1 - width(&label) as u16;
                pic.put(label_x, y, &format!("{label}─"), STANDARD);
                label_x
            };
            pic.put(end, y, "┼", border);
            let (mark, style) = match marker {
                Marker::OffFlow { broken: false } => ("─▣", STANDARD),
                Marker::OffFlow { broken: true } => ("─▣", ERROR),
                _ => ("─", STANDARD),
            };
            pic.put(end + 1, y, mark, style);
            let label = (label_x, width(&label) as u16);
            outputs.push((row.clone(), PlacedRow { y, label }));
            y += 1;
        }
    }
    pic.put(x, y, &format!("╰{}╯", "─".repeat(inner)), border);
    Placed {
        node: b.id.clone(),
        x,
        width: b.width(),
        height: b.height(),
        inputs,
        outputs,
    }
}

/// Terminal colour for a swatch, transparency blended over black.
fn rgb(c: Rgba<u8>) -> Color {
    let a = u16::from(c[3]);
    let mix = |v: u8| (u16::from(v) * a / 255) as u8;
    Color::Rgb(mix(c[0]), mix(c[1]), mix(c[2]))
}

/// A conduit from the cell after an output's `┼` to an input's `┼` (not included).
/// Returns its cells.
fn draw_conduit(pic: &mut Picture, (x0, y0): (u16, u16), (x1, y1): (u16, u16)) -> Vec<(u16, u16)> {
    let mut cells = Vec::new();
    let mut put = |pic: &mut Picture, x: u16, y: u16, text: &str| {
        pic.put(x, y, text, STANDARD);
        cells.extend((0..width(text) as u16).map(|i| (x + i, y)));
    };
    if y0 == y1 {
        put(pic, x0, y0, &"─".repeat((x1 - x0) as usize));
        return cells;
    }
    // Bend two columns out, clear of other rows' `▣` markers.
    let up = y1 < y0;
    let turn = x0 + 2;
    put(pic, x0, y0, if up { "──╯" } else { "──╮" });
    for y in y0.min(y1) + 1..y0.max(y1) {
        put(pic, turn, y, "│");
    }
    let run = "─".repeat((x1 - turn - 1) as usize);
    put(
        pic,
        turn,
        y1,
        &format!("{}{run}", if up { "╭" } else { "╰" }),
    );
    cells
}

/// Colours the focus green and what it's related to magenta (§5.2, §5.3). Conduit
/// lines are only ever coloured as conduits, and swatches keep their colour.
fn highlight(pic: &mut Picture, flow: &Flow, focus: &Focus) {
    let Some(i) = flow.position(focus.node()) else {
        return;
    };
    let lines: HashSet<(u16, u16)> = pic.conduits.iter().flatten().copied().collect();
    let paint = |pic: &mut Picture, x: u16, y: u16, len: u16, style: Style| {
        for x in x..x.saturating_add(len) {
            if lines.contains(&(x, y)) {
                continue;
            }
            if let Some(cell) = pic.cell_mut(x, y)
                && cell.ch != '█'
            {
                cell.style = style;
            }
        }
    };
    let conduit = |pic: &mut Picture, j: usize, style: Style| {
        for (x, y) in pic.conduits.get(j).cloned().unwrap_or_default() {
            if let Some(cell) = pic.cell_mut(x, y) {
                cell.style = style;
            }
        }
    };
    let label = |pic: &mut Picture, r: PlacedRow, style: Style| {
        paint(pic, r.label.0, r.y, r.label.1, style);
    };
    let b = pic.boxes[i].clone();
    // The far end of a conduit end, when its node is on the flow: (box, row).
    let far_input = |pic: &Picture, node: &str, port: &str, index: usize| {
        let k = flow.position(node)?;
        pic.boxes[k]
            .inputs
            .iter()
            .find(|(r, _)| {
                matches!(r.state, InState::Connected(_))
                    && r.port == port
                    && r.index.unwrap_or(0) == index
            })
            .map(|(_, p)| *p)
    };
    let far_output =
        |pic: &Picture, user_of: &str, node: &str, port: &str, index: usize, output: &str| {
            let k = flow.position(user_of)?;
            pic.boxes[k]
                .outputs
                .iter()
                .find(|(r, _)| {
                    r.port == output
                        && r.user
                            .as_ref()
                            .is_some_and(|u| u.node == node && u.port == port && u.index == index)
                })
                .map(|(_, p)| *p)
        };

    match focus {
        Focus::Node(_) => {
            if i > 0 {
                conduit(pic, i - 1, RELATED);
            }
            conduit(pic, i, RELATED);
            for y in TOP..TOP + b.height {
                paint(pic, b.x, y, b.width, FOCUS);
            }
            for (_, r) in &b.inputs {
                paint(pic, b.x.saturating_sub(2), r.y, 2, FOCUS);
            }
            for (_, r) in &b.outputs {
                paint(pic, b.x + b.width, r.y, 2, FOCUS);
            }
        }
        Focus::Conduit(link) => {
            conduit(pic, i - 1, FOCUS);
            if let Some(r) = far_input(pic, &link.target, &link.port, link.index) {
                label(pic, r, RELATED);
            }
            let source = (
                &link.source,
                &link.target,
                &link.port,
                link.index,
                &link.output,
            );
            if let Some(r) = far_output(pic, source.0, source.1, source.2, source.3, source.4) {
                label(pic, r, RELATED);
            }
        }
        Focus::In { node, row } => {
            let Some((_, r)) = b.inputs.iter().find(|(x, _)| same_input(x, row)) else {
                return;
            };
            let r = *r;
            paint(pic, b.x.saturating_sub(2), r.y, 4, FOCUS);
            label(pic, r, RELATED);
            if let InState::Connected(source) = &row.state {
                let index = row.index.unwrap_or(0);
                if let Some(far) =
                    far_output(pic, &source.node, node, &row.port, index, &source.port)
                {
                    label(pic, far, RELATED);
                }
            }
        }
        Focus::Out { row, .. } => {
            let Some((_, r)) = b.outputs.iter().find(|(x, _)| same_output(x, row)) else {
                return;
            };
            let r = *r;
            paint(pic, b.x + b.width - 2, r.y, 4, FOCUS);
            label(pic, r, RELATED);
            if let Some(user) = &row.user
                && let Some(far) = far_input(pic, &user.node, &user.port, user.index)
            {
                label(pic, far, RELATED);
            }
        }
    }
}

/// Which part of the picture is on screen (§4.5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Viewport {
    pub x: u16,
    pub y: u16,
}

impl Viewport {
    /// Scrolls to show `focus` in a `w × h` area. `recentre` after a flow change;
    /// otherwise as little as possible, keeping a box to spare on each side where it
    /// fits. Never past the picture's edges.
    pub fn follow(
        self,
        pic: &Picture,
        flow: &Flow,
        focus: &Focus,
        w: u16,
        h: u16,
        recentre: bool,
    ) -> Self {
        let Some(i) = flow.position(focus.node()) else {
            return Self::default();
        };
        let Some(b) = pic.boxes.get(i) else {
            return Self::default();
        };
        let span = |b: &Placed| (b.x.saturating_sub(2), b.x + b.width + 2);
        // The focus's columns and rows, and the neighbours to keep in view if they fit.
        let (mut x0, mut x1) = span(b);
        let (mut spare0, mut spare1) = (
            i.checked_sub(1)
                .and_then(|k| pic.boxes.get(k))
                .map_or(x0, |p| span(p).0),
            pic.boxes.get(i + 1).map_or(x1, |n| span(n).1),
        );
        let (y0, y1) = match focus {
            Focus::Node(_) => (0, TOP + b.height),
            Focus::Conduit(_) => {
                let p = &pic.boxes[i - 1];
                (x0, x1) = (p.x + p.width, b.x);
                (spare0, spare1) = (span(p).0, span(b).1);
                let ys: Vec<u16> = pic.conduits[i - 1].iter().map(|c| c.1).collect();
                let lo = ys.iter().copied().min().unwrap_or(TOP);
                (lo, ys.iter().copied().max().unwrap_or(TOP) + 1)
            }
            Focus::In { row, .. } => b
                .inputs
                .iter()
                .find(|(r, _)| same_input(r, row))
                .map_or((0, TOP), |(_, p)| (p.y, p.y + 1)),
            Focus::Out { row, .. } => b
                .outputs
                .iter()
                .find(|(r, _)| same_output(r, row))
                .map_or((0, TOP), |(_, p)| (p.y, p.y + 1)),
        };
        let x = if recentre {
            ((x0 + x1) / 2).saturating_sub(w / 2)
        } else if spare1 - spare0 <= w {
            minimal(self.x, spare0, spare1, w)
        } else {
            minimal(self.x, x0, x1, w)
        };
        let from = if recentre { 0 } else { self.y };
        let y = minimal(from, y0, y1.min(y0 + h), h);
        Self {
            x: x.min(pic.width.saturating_sub(w)),
            y: y.min(pic.height.saturating_sub(h)),
        }
    }
}

/// The smallest change to `offset` that shows `[from, to)` in `size` cells.
fn minimal(offset: u16, from: u16, to: u16, size: u16) -> u16 {
    if from < offset {
        from
    } else if to > offset + size {
        (to - size).min(from)
    } else {
        offset
    }
}

#[cfg(test)]
mod tests {
    use super::super::flow::tests::example;
    use super::*;

    fn node(id: &str) -> Focus {
        Focus::Node(id.into())
    }

    #[test]
    fn draws_story_two_opening_screen() {
        // docs/stories/02-exploring.md, step 1 (the canvas rows).
        let project = example("effects");
        let flow = Flow::build(&project, "preview");
        let pic = draw(&project, &flow, Some(&node("preview")));
        let expected = "


      ╭─ canvas ─╮      ╭─ composite ─╮      ╭─ preview ─╮
      │ bg       │      │ comp        │      │ preview   │
      │──────────│      │─────────────│      │───────────│
    ·─┼─image    │  ╭───┼─base        │  ╭───┼─image     │
      │──────────│  │ ▣─┼─layers[0]   │  │   ╰───────────╯
      │      out─┼──╯ ▣─┼─layers[1]   │  │
      ╰──────────╯    ▣─┼─layers[2]   │  │
                      ·─┼─layers      │  │
                        │─────────────│  │
                        │         out─┼──╯
                        ╰─────────────╯";
        assert_eq!(pic.text(), &expected[1..]);
    }

    #[test]
    fn focus_is_green_and_its_conduits_magenta() {
        let project = example("effects");
        let flow = Flow::build(&project, "preview");
        let pic = draw(&project, &flow, Some(&node("preview")));
        let preview = &pic.boxes[2];
        assert_eq!(pic.style_at(preview.x, TOP), FOCUS);
        // comp → preview is related; bg → comp isn't.
        let y = preview.inputs[0].1.y;
        assert_eq!(pic.style_at(preview.x - 1, y), RELATED);
        assert_eq!(
            pic.style_at(pic.boxes[0].x + pic.boxes[0].width, TOP + 3),
            STANDARD
        );
        // The `▣` markers on comp stay white.
        let y = pic.boxes[1].inputs[1].1.y;
        assert_eq!(pic.style_at(pic.boxes[1].x - 2, y), STANDARD);
    }

    #[test]
    fn conduit_and_end_focus_colours() {
        let project = example("effects");
        let flow = Flow::build(&project, "preview");
        // The conduit comp → preview: the line green, the two port labels magenta.
        let link = flow.links[1].clone();
        let pic = draw(&project, &flow, Some(&Focus::Conduit(link)));
        let (x, y) = pic.conduits[1][0];
        assert_eq!(pic.style_at(x, y), FOCUS);
        let comp = &pic.boxes[1];
        let preview = &pic.boxes[2];
        let out = comp.outputs[0].1;
        assert_eq!(pic.style_at(out.label.0, out.y), RELATED);
        assert_eq!(pic.style_at(comp.x + comp.width - 1, out.y), STANDARD); // `┼`
        let image = preview.inputs[0].1;
        assert_eq!(pic.style_at(image.label.0, image.y), RELATED);
        assert_eq!(pic.style_at(comp.x, TOP), STANDARD);

        // A `▣` end: its marker and stub green, its label magenta, the box white.
        let (row, placed) = comp.inputs[1].clone();
        let focus = Focus::In {
            node: "comp".into(),
            row,
        };
        let pic = draw(&project, &flow, Some(&focus));
        assert_eq!(pic.style_at(comp.x - 2, placed.y), FOCUS);
        assert_eq!(pic.style_at(comp.x, placed.y), FOCUS);
        assert_eq!(pic.style_at(placed.label.0, placed.y), RELATED);
        assert_eq!(pic.style_at(comp.x, TOP), STANDARD);

        // The in-flow end `base`: the line into it stays white.
        let (row, placed) = comp.inputs[0].clone();
        let focus = Focus::In {
            node: "comp".into(),
            row,
        };
        let pic = draw(&project, &flow, Some(&focus));
        assert_eq!(pic.style_at(comp.x - 1, placed.y), STANDARD);
        assert_eq!(pic.style_at(comp.x, placed.y), FOCUS);
        // ... and the far end, `bg.out`, is magenta.
        let bg_out = pic.boxes[0].outputs[0].1;
        assert_eq!(pic.style_at(bg_out.label.0, bg_out.y), RELATED);
    }

    #[test]
    fn palette_swatches_line_up() {
        let project = example("palette");
        let flow = Flow::build(&project, "brand");
        let pic = draw(&project, &flow, None);
        let text = pic.text();
        let lines: Vec<&str> = text
            .lines()
            .map(|l| &l[..l.find('┼').unwrap_or(0)])
            .collect();
        assert!(lines.contains(&"    │ ██ primary─"));
        assert!(lines.contains(&"    │ ██ teal────"));
    }

    #[test]
    fn long_names_are_cut() {
        assert_eq!(fit("helloWorldTransformation", 18), "helloWorldTransfo…");
        assert_eq!(fit("short", 18), "short");
    }

    #[test]
    fn scrolls_minimally_or_recentres() {
        let project = example("effects");
        let flow = Flow::build(&project, "logo_tinted");
        let pic = draw(&project, &flow, None);
        assert!(pic.width > 80);
        let n = flow.nodes.len();
        let at = |i: usize| node(&flow.nodes[i]);
        // Re-centring on the last box stops at the flow's end.
        let v = Viewport::default().follow(&pic, &flow, &at(n - 1), 80, 16, true);
        assert_eq!(v.x, pic.width - 80);
        // Walking back one box from there doesn't move the view.
        let w = v.follow(&pic, &flow, &at(n - 2), 80, 16, false);
        assert_eq!(w.x, v.x);
        // The first box is always fully visible from the start.
        let v = Viewport::default().follow(&pic, &flow, &at(0), 80, 16, false);
        assert_eq!(v.x, 0);
    }
}
