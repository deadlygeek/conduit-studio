//! Draws a [`Flow`] (spec §3, §4): node boxes left to right, joined by the flow's
//! conduits, with `▣` / `·` / `✕` markers on the other rows. The whole flow is drawn into
//! a [`Picture`] at its full size; [`Viewport`] decides which part of it is on screen.

use conduit_core::{Project, ProjectNode, Rgba, parse_color};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use super::flow::{Flow, InRow, InState, OutRow, input_rows, output_rows};

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
    /// Each input row and its picture row.
    pub inputs: Vec<(InRow, u16)>,
    /// Each output row and its picture row.
    pub outputs: Vec<(OutRow, u16)>,
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
}

impl Picture {
    fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            cells: vec![BLANK; width as usize * height as usize],
            boxes: Vec::new(),
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

/// Draws `flow`. `focus` is the focused node, drawn green with its flow conduits magenta
/// (§5.2, §5.3).
pub fn draw(project: &Project, flow: &Flow, focus: Option<&str>) -> Picture {
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
        if let Some((from_y, from_x)) = prev {
            let to_y = placed
                .inputs
                .iter()
                .zip(&b.inputs)
                .find(|(_, (_, _, m))| *m == Marker::Flow)
                .map(|((_, y), _)| *y);
            if let Some(to_y) = to_y {
                let related = focus.is_some_and(|f| f == b.id || f == boxes[i - 1].id);
                let style = if related { RELATED } else { STANDARD };
                draw_conduit(&mut pic, (from_x, from_y), (x, to_y), style);
            }
        }
        prev = placed
            .outputs
            .iter()
            .zip(&b.outputs)
            .find(|(_, (_, _, m, _))| *m == Marker::Flow)
            .map(|((_, y), _)| (*y, x + b.width()));
        if focus == Some(b.id.as_str()) {
            focus_box(&mut pic, &placed);
        }
        pic.boxes.push(placed);
        x += b.width() + GAP;
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
            pic.put(x, y, "┼─", border);
            pic.put(
                x + 2,
                y,
                &fit(label, content),
                if dim { DIM } else { STANDARD },
            );
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
            inputs.push((row.clone(), y));
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
            if let Some(c) = colour {
                // A palette colour (§7.1): swatches in a column, `██ name───┼`.
                pic.put(x + 2, y, "██", STANDARD.fg(rgb(*c)));
                let dashes = "─".repeat(inner - 4 - width(&label));
                pic.put(x + 5, y, &format!("{label}{dashes}"), STANDARD);
            } else {
                let label_x = end - 1 - width(&label) as u16;
                pic.put(label_x, y, &format!("{label}─"), STANDARD);
            }
            pic.put(end, y, "┼", border);
            let (mark, style) = match marker {
                Marker::OffFlow { broken: false } => ("─▣", STANDARD),
                Marker::OffFlow { broken: true } => ("─▣", ERROR),
                _ => ("─", STANDARD),
            };
            pic.put(end + 1, y, mark, style);
            outputs.push((row.clone(), y));
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
fn draw_conduit(pic: &mut Picture, (x0, y0): (u16, u16), (x1, y1): (u16, u16), style: Style) {
    if y0 == y1 {
        pic.put(x0, y0, &"─".repeat((x1 - x0) as usize), style);
        return;
    }
    // Bend two columns out, clear of other rows' `▣` markers.
    let up = y1 < y0;
    let turn = x0 + 2;
    pic.put(x0, y0, if up { "──╯" } else { "──╮" }, style);
    for y in y0.min(y1) + 1..y0.max(y1) {
        pic.put(turn, y, "│", style);
    }
    let run = "─".repeat((x1 - turn - 1) as usize);
    pic.put(
        turn,
        y1,
        &format!("{}{run}", if up { "╭" } else { "╰" }),
        style,
    );
}

/// Turns a box green: borders, text, stubs and markers (§5.2). Swatches keep their
/// colour, and the flow conduits leaving the box keep theirs.
fn focus_box(pic: &mut Picture, b: &Placed) {
    let paint = |pic: &mut Picture, x: u16, y: u16, len: u16| {
        for x in x..(x + len).min(pic.width) {
            let cell = &mut pic.cells[y as usize * pic.width as usize + x as usize];
            if cell.ch != '█' && cell.style != RELATED {
                cell.style = FOCUS;
            }
        }
    };
    for y in TOP..TOP + b.height {
        paint(pic, b.x, y, b.width);
    }
    for (_, y) in &b.inputs {
        paint(pic, b.x.saturating_sub(2), *y, 2);
    }
    for (_, y) in &b.outputs {
        paint(pic, b.x + b.width, *y, 2);
    }
}

/// Which part of the picture is on screen (§4.5).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Viewport {
    pub x: u16,
    pub y: u16,
}

impl Viewport {
    /// Scrolls to show `pic.boxes[focus]` in a `w × h` area. `recentre` after a flow
    /// change; otherwise as little as possible, keeping a box to spare on each side
    /// where it fits. Never past the picture's edges.
    pub fn follow(self, pic: &Picture, focus: usize, w: u16, h: u16, recentre: bool) -> Self {
        let Some(b) = pic.boxes.get(focus) else {
            return Self::default();
        };
        let max_x = pic.width.saturating_sub(w);
        let max_y = pic.height.saturating_sub(h);
        let (left, right) = (b.x.saturating_sub(2), b.x + b.width + 2);
        let x = if recentre {
            ((left + right) / 2).saturating_sub(w / 2)
        } else {
            // A box to spare on each side, if that fits; else just the focus.
            let spare_left = focus
                .checked_sub(1)
                .and_then(|i| pic.boxes.get(i))
                .map_or(left, |p| p.x.saturating_sub(2));
            let spare_right = pic
                .boxes
                .get(focus + 1)
                .map_or(right, |n| n.x + n.width + 2);
            let (l, r) = if spare_right - spare_left <= w {
                (spare_left, spare_right)
            } else {
                (left, right)
            };
            minimal(self.x, l, r, w)
        };
        // Vertically: from the margin above the box, as much of it as fits.
        let from = if recentre { 0 } else { self.y };
        let y = minimal(from, 0, (TOP + b.height).min(h), h);
        Self {
            x: x.min(max_x),
            y: y.min(max_y),
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

    #[test]
    fn draws_story_two_opening_screen() {
        // docs/stories/02-exploring.md, step 1 (the canvas rows).
        let project = example("effects");
        let flow = Flow::build(&project, "preview");
        let pic = draw(&project, &flow, Some("preview"));
        let expected = "


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
                      ╰─────────────╯";
        assert_eq!(pic.text(), &expected[1..]);
    }

    #[test]
    fn focus_is_green_and_its_conduits_magenta() {
        let project = example("effects");
        let flow = Flow::build(&project, "preview");
        let pic = draw(&project, &flow, Some("preview"));
        let preview = &pic.boxes[2];
        assert_eq!(pic.style_at(preview.x, TOP), FOCUS);
        // comp → preview is related; bg → comp isn't.
        let (_, y) = preview.inputs[0];
        assert_eq!(pic.style_at(preview.x - 1, y), RELATED);
        assert_eq!(
            pic.style_at(pic.boxes[0].x + pic.boxes[0].width, TOP + 3),
            STANDARD
        );
        // The `▣` markers on comp stay white.
        let (_, y) = pic.boxes[1].inputs[1];
        assert_eq!(pic.style_at(pic.boxes[1].x - 2, y), STANDARD);
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
        let last = pic.boxes.len() - 1;
        // Re-centring on the last box stops at the flow's end.
        let v = Viewport::default().follow(&pic, last, 80, 16, true);
        assert_eq!(v.x, pic.width - 80);
        // Walking back one box from there doesn't move the view.
        let w = v.follow(&pic, last - 1, 80, 16, false);
        assert_eq!(w.x, v.x);
        // The first box is always fully visible from the start.
        let v = Viewport::default().follow(&pic, 0, 80, 16, false);
        assert_eq!(v.x, 0);
    }
}
