//! Editor state and key handling, independent of the terminal (see `view.rs` for drawing).

mod colors;
mod edits;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use conduit_core::{Change, ParamKind, Project, ProjectNode, Reference, Registry, TreeItem};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

use crate::run::{Event, Shown};
pub use colors::ColorEdit;
pub use edits::{Confirm, Pick, Prompt};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Up,
    Node,
    Down,
}

impl Pane {
    fn index(self) -> usize {
        self as usize
    }
}

/// One line of the focused node's panel.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    /// An input connection (`index` is the position in a list input).
    Input {
        port: String,
        index: Option<usize>,
        source: Option<Reference>,
    },
    /// A param; number params may instead be linked to another node's output.
    Param {
        name: String,
        link: Option<Reference>,
    },
    /// One entry of a table param (a palette color), which is also an output of its own.
    Entry {
        param: String,
        key: String,
    },
    Output {
        port: String,
    },
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Normal,
    /// Choosing from a filtered list (find, connect, node types).
    Pick {
        purpose: Pick,
        query: String,
        selected: usize,
    },
    /// Typing a value; `error` is shown when the last submission was rejected.
    Prompt {
        purpose: Prompt,
        input: String,
        error: Option<String>,
    },
    /// Waiting for `y` to go ahead.
    Confirm {
        question: String,
        purpose: Confirm,
    },
    Help,
    /// The color editor overlay.
    Color(Box<ColorEdit>),
}

/// What the event loop should do after a key.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Quit,
    Probe(Reference),
    ProbeOff,
    /// Suspend the TUI and open this file in `$EDITOR`.
    OpenEditor(PathBuf),
}

pub struct App {
    pub dir: PathBuf,
    pub project: Project,
    registry: Arc<Registry>,
    /// Edits made in this session, most recent last.
    undo: Vec<(String, Change)>,
    /// `node.param` being nudged, while the last undo entry is its nudges (they undo as one).
    nudging: Option<String>,
    pub focus: Option<String>,
    pub pane: Pane,
    pub mode: Mode,
    /// Rows of the focused node.
    pub rows: Vec<Row>,
    /// Lineage trees of the focused node (full; collapsing is applied by `visible`).
    pub up: Vec<TreeItem>,
    pub down: Vec<TreeItem>,
    collapsed: [BTreeSet<String>; 2],
    /// Cursor and scroll position for each pane.
    pub lists: [ListState; 3],
    back: Vec<String>,
    forward: Vec<String>,
    /// Latest runner status (render result or warning).
    pub status: Option<Event>,
    /// One-off feedback, cleared by the next key.
    pub message: Option<String>,
    pub probe: Option<Reference>,
    pub probe_follow: bool,
}

impl App {
    pub fn new(dir: PathBuf, project: Project, registry: Arc<Registry>) -> Self {
        let mut app = Self {
            dir,
            project,
            registry,
            undo: Vec::new(),
            nudging: None,
            focus: None,
            pane: Pane::Node,
            mode: Mode::Normal,
            rows: Vec::new(),
            up: Vec::new(),
            down: Vec::new(),
            collapsed: Default::default(),
            lists: Default::default(),
            back: Vec::new(),
            forward: Vec::new(),
            status: None,
            message: None,
            probe: None,
            probe_follow: false,
        };
        app.focus = app.default_focus();
        app.refresh();
        app
    }

    /// Start where the results are: a GUI node, else a node nothing consumes.
    fn default_focus(&self) -> Option<String> {
        let nodes = || self.project.nodes.values();
        nodes()
            .find(|n| n.spec.as_ref().is_some_and(|s| s.gui))
            .or_else(|| nodes().find(|n| self.project.consumers(&n.id).is_empty()))
            .or_else(|| nodes().next())
            .map(|n| n.id.clone())
    }

    pub fn focused(&self) -> Option<&ProjectNode> {
        self.focus.as_deref().and_then(|id| self.project.get(id))
    }

    /// Replaces the project after files changed, keeping focus and history where possible.
    /// History entries for nodes that no longer exist are skipped when navigating, so a
    /// rename can re-point them and an undo can bring them back.
    pub fn reload(&mut self, project: Project) {
        self.project = project;
        if !self
            .focus
            .as_ref()
            .is_some_and(|id| self.project.nodes.contains_key(id))
        {
            self.focus = self.default_focus();
        }
        self.refresh();
    }

    pub fn on_runner_event(&mut self, event: Event) {
        if event != Event::FilesChanged {
            self.status = Some(event);
        }
    }

    /// The value of a number or color output from the last successful render, if any.
    pub fn linked_value(&self, source: &Reference) -> Option<Shown> {
        match &self.status {
            Some(Event::Rendered { shown, .. }) => shown.get(source).copied(),
            _ => None,
        }
    }

    /// Rebuilds rows and trees for the focused node and keeps cursors in range.
    fn refresh(&mut self) {
        let (rows, up, down) = match self.focused() {
            Some(node) => (
                node_rows(&self.project, node),
                self.project.upstream_tree(&node.id, None),
                self.project.downstream_tree(&node.id, None),
            ),
            None => Default::default(),
        };
        self.rows = rows;
        self.up = up;
        self.down = down;
        for pane in [Pane::Up, Pane::Node, Pane::Down] {
            let len = self.pane_len(pane);
            let list = &mut self.lists[pane.index()];
            let selected = list.selected().unwrap_or(0).min(len.saturating_sub(1));
            list.select((len > 0).then_some(selected));
        }
    }

    /// Indices into the pane's tree of the items not hidden by a collapsed ancestor.
    pub fn visible(&self, pane: Pane) -> Vec<usize> {
        let (items, collapsed) = match pane {
            Pane::Up => (&self.up, &self.collapsed[0]),
            Pane::Down => (&self.down, &self.collapsed[1]),
            Pane::Node => return (0..self.rows.len()).collect(),
        };
        let mut out = Vec::new();
        let mut hide_below: Option<usize> = None;
        for (i, item) in items.iter().enumerate() {
            if hide_below.is_some_and(|d| item.depth > d) {
                continue;
            }
            hide_below = None;
            out.push(i);
            if collapsed.contains(&item.node) {
                hide_below = Some(item.depth);
            }
        }
        out
    }

    pub fn is_collapsed(&self, pane: Pane, node: &str) -> bool {
        match pane {
            Pane::Up => self.collapsed[0].contains(node),
            Pane::Down => self.collapsed[1].contains(node),
            Pane::Node => false,
        }
    }

    fn pane_len(&self, pane: Pane) -> usize {
        self.visible(pane).len()
    }

    pub fn cursor(&self, pane: Pane) -> usize {
        self.lists[pane.index()].selected().unwrap_or(0)
    }

    fn set_cursor(&mut self, pane: Pane, i: usize) {
        let len = self.pane_len(pane);
        self.lists[pane.index()].select((len > 0).then_some(i.min(len.saturating_sub(1))));
    }

    fn current_row(&self) -> Option<&Row> {
        self.rows.get(self.cursor(Pane::Node))
    }

    /// The tree item under the cursor in a side pane.
    fn current_item(&self) -> Option<&TreeItem> {
        let tree = match self.pane {
            Pane::Up => &self.up,
            Pane::Down => &self.down,
            Pane::Node => return None,
        };
        self.visible(self.pane)
            .get(self.cursor(self.pane))
            .map(|&i| &tree[i])
    }

    // --- focus and history -------------------------------------------------------------

    fn focus_on(&mut self, id: &str) -> Option<Action> {
        if !self.project.nodes.contains_key(id) {
            self.message = Some(format!("`{id}` doesn't exist"));
            return None;
        }
        if self.focus.as_deref() == Some(id) {
            self.pane = Pane::Node;
            return None;
        }
        if let Some(old) = self.focus.take() {
            self.back.push(old);
        }
        self.forward.clear();
        self.set_focus(id)
    }

    fn set_focus(&mut self, id: &str) -> Option<Action> {
        self.focus = Some(id.to_string());
        self.pane = Pane::Node;
        self.lists = Default::default();
        self.refresh();
        self.follow_probe()
    }

    fn go_back(&mut self) -> Option<Action> {
        let id = pop_existing(&mut self.back, &self.project)?;
        if let Some(cur) = self.focus.take() {
            self.forward.push(cur);
        }
        self.set_focus(&id)
    }

    fn go_forward(&mut self) -> Option<Action> {
        let id = pop_existing(&mut self.forward, &self.project)?;
        if let Some(cur) = self.focus.take() {
            self.back.push(cur);
        }
        self.set_focus(&id)
    }

    /// `h` in the node panel: go to the source of the input under the cursor (or the first
    /// connected input), landing on the output we came through.
    fn follow_upstream(&mut self) -> Option<Action> {
        let source = row_source(self.current_row())
            .or_else(|| self.rows.iter().find_map(|r| row_source(Some(r))))
            .cloned();
        let Some(source) = source else {
            self.message = Some("no connected inputs".into());
            return None;
        };
        let action = self.focus_on(&source.node);
        self.select_row(|r| row_output(r) == Some(source.port.as_str()));
        action
    }

    /// `l` in the node panel: go to the first consumer of the output under the cursor (or
    /// of any output), landing on the input we came through.
    fn follow_downstream(&mut self) -> Option<Action> {
        let id = self.focus.clone()?;
        let consumers = self.project.consumers(&id);
        let consumer = match self.current_row() {
            Some(row) => {
                row_output(row).and_then(|port| consumers.iter().find(|c| c.output == port))
            }
            None => None,
        }
        .or(consumers.first())
        .cloned();
        let Some(c) = consumer else {
            self.message = Some("nothing uses this node".into());
            return None;
        };
        let action = self.focus_on(&c.node);
        let multi = self
            .focused()
            .and_then(|n| n.spec.as_ref())
            .and_then(|s| s.input(&c.port))
            .is_some_and(|p| p.multi);
        let index = multi.then_some(c.index);
        self.select_row(|r| match r {
            Row::Input { port, index: i, .. } => *port == c.port && *i == index,
            Row::Param { name, .. } => *name == c.port,
            _ => false,
        });
        action
    }

    fn select_row(&mut self, pred: impl Fn(&Row) -> bool) {
        if let Some(i) = self.rows.iter().position(pred) {
            self.set_cursor(Pane::Node, i);
        }
    }

    // --- probe -------------------------------------------------------------------------

    /// What `p` probes: the connection or output under the cursor, else the focused node's
    /// first output, else the source of its first connected input.
    fn probe_target(&self) -> Option<Reference> {
        if let Some(item) = self.current_item() {
            return self.default_probe(&item.node);
        }
        if let Some(source) = row_source(self.current_row()) {
            return Some(source.clone());
        }
        match self.current_row().and_then(row_output) {
            Some(port) => Some(Reference {
                node: self.focus.clone()?,
                port: port.to_string(),
            }),
            _ => self.default_probe(self.focus.as_deref()?),
        }
    }

    fn default_probe(&self, id: &str) -> Option<Reference> {
        let node = self.project.get(id)?;
        if let Some((port, _)) = node.outputs().into_iter().next() {
            return Some(Reference {
                node: id.to_string(),
                port,
            });
        }
        node.input_ports()
            .into_iter()
            .find_map(|(_, refs)| refs.into_iter().next())
    }

    fn probe(&mut self, target: Option<Reference>) -> Option<Action> {
        let Some(target) = target else {
            self.message = Some("nothing to probe here".into());
            return None;
        };
        self.probe = Some(target.clone());
        Some(Action::Probe(target))
    }

    fn follow_probe(&mut self) -> Option<Action> {
        if !self.probe_follow {
            return None;
        }
        let target = self.default_probe(self.focus.as_deref()?);
        self.probe(target)
    }

    // --- find --------------------------------------------------------------------------

    // --- keys --------------------------------------------------------------------------

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        self.message = None;
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Some(Action::Quit);
        }
        match std::mem::replace(&mut self.mode, Mode::Normal) {
            Mode::Normal => self.normal_key(key),
            Mode::Help => None,
            Mode::Pick {
                purpose,
                query,
                selected,
            } => self.pick_key(key, purpose, query, selected),
            Mode::Prompt {
                purpose,
                input,
                error,
            } => self.prompt_key(key, purpose, input, error),
            Mode::Color(edit) => {
                self.color_key(key, edit);
                None
            }
            Mode::Confirm { question, purpose } => {
                if key.code == KeyCode::Char('y') {
                    self.confirmed(purpose);
                } else {
                    self.message = Some(format!("cancelled: {question}"));
                }
                None
            }
        }
    }

    /// Keys while a picker is open (mode has been reset to `Normal`; restore to stay open).
    fn pick_key(
        &mut self,
        key: KeyEvent,
        purpose: Pick,
        mut query: String,
        mut selected: usize,
    ) -> Option<Action> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return None,
            KeyCode::Enter => {
                self.mode = Mode::Pick {
                    purpose: purpose.clone(),
                    query,
                    selected,
                };
                let chosen = self.pick_matches().into_iter().nth(selected);
                self.mode = Mode::Normal;
                return chosen.and_then(|(key, _)| self.choose(purpose, key));
            }
            KeyCode::Down => selected += 1,
            KeyCode::Char('n') if ctrl => selected += 1,
            KeyCode::Up => selected = selected.saturating_sub(1),
            KeyCode::Char('p') if ctrl => selected = selected.saturating_sub(1),
            KeyCode::Backspace => {
                query.pop();
                selected = 0;
            }
            KeyCode::Char(c) if !ctrl => {
                query.push(c);
                selected = 0;
            }
            _ => {}
        }
        self.mode = Mode::Pick {
            purpose,
            query,
            selected,
        };
        let count = self.pick_matches().len();
        if let Mode::Pick { selected, .. } = &mut self.mode {
            *selected = (*selected).min(count.saturating_sub(1));
        }
        None
    }

    /// Keys while a prompt is open.
    fn prompt_key(
        &mut self,
        key: KeyEvent,
        purpose: Prompt,
        mut input: String,
        mut error: Option<String>,
    ) -> Option<Action> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return None,
            KeyCode::Enter => match self.submit_prompt(purpose.clone(), &input) {
                Ok(action) => return action,
                Err(e) => error = Some(e),
            },
            KeyCode::Backspace => {
                input.pop();
            }
            KeyCode::Char('u') if ctrl => input.clear(),
            KeyCode::Char(c) if !ctrl => input.push(c),
            _ => {}
        }
        self.mode = Mode::Prompt {
            purpose,
            input,
            error,
        };
        None
    }

    fn normal_key(&mut self, key: KeyEvent) -> Option<Action> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match (self.pane, key.code) {
            (_, KeyCode::Char('q')) => return Some(Action::Quit),
            (_, KeyCode::Char('?')) => self.mode = Mode::Help,
            (_, KeyCode::Char('/')) => self.open_pick(Pick::FindNode),

            // Editing.
            (Pane::Node, KeyCode::Enter) => self.edit_row(),
            (Pane::Node, KeyCode::Char('c')) => self.connect_row(),
            (Pane::Node, KeyCode::Char('d')) => self.disconnect_row(),
            (Pane::Node, KeyCode::Char('i')) => self.insert_row(),
            (Pane::Node, KeyCode::Char(c)) if nudge_step(c).is_some() => {
                self.nudge_row(nudge_step(c).expect("checked"))
            }
            (_, KeyCode::Char('a')) => self.open_pick(Pick::AddType),
            (Pane::Node, KeyCode::Char('o')) if !ctrl => self.start_add_entry(),
            (Pane::Node, KeyCode::Char('x')) => self.start_remove_entry(),
            (_, KeyCode::Char('I')) => match self.entries_param() {
                Some(param) => self.open_prompt(Prompt::Import { param }, String::new()),
                None => self.message = Some("I imports colors into a palette".into()),
            },
            (_, KeyCode::Char('r')) => self.start_rename(),
            (_, KeyCode::Char('D')) => self.start_delete(),
            (_, KeyCode::Char('u')) => self.undo(),
            (_, KeyCode::Char('E')) => {
                return self.focused().map(|n| Action::OpenEditor(n.path.clone()));
            }

            (_, KeyCode::Char('o')) if ctrl => return self.go_back(),
            (_, KeyCode::Char('[') | KeyCode::Backspace) => return self.go_back(),
            (_, KeyCode::Char(']')) => return self.go_forward(),

            (pane, KeyCode::Char('j') | KeyCode::Down) => {
                self.set_cursor(pane, self.cursor(pane) + 1)
            }
            (pane, KeyCode::Char('k') | KeyCode::Up) => {
                self.set_cursor(pane, self.cursor(pane).saturating_sub(1))
            }
            (pane, KeyCode::Char('g') | KeyCode::Home) => self.set_cursor(pane, 0),
            (pane, KeyCode::Char('G') | KeyCode::End) => self.set_cursor(pane, usize::MAX),

            (_, KeyCode::Char('H')) => self.pane = Pane::Up,
            (_, KeyCode::Char('L')) => self.pane = Pane::Down,
            (_, KeyCode::Esc) => self.pane = Pane::Node,
            (pane, KeyCode::Tab) => {
                self.pane = match pane {
                    Pane::Up => Pane::Node,
                    Pane::Node => Pane::Down,
                    Pane::Down => Pane::Up,
                }
            }
            (pane, KeyCode::BackTab) => {
                self.pane = match pane {
                    Pane::Up => Pane::Down,
                    Pane::Node => Pane::Up,
                    Pane::Down => Pane::Node,
                }
            }

            (Pane::Node, KeyCode::Char('h') | KeyCode::Left) => return self.follow_upstream(),
            (Pane::Node, KeyCode::Char('l') | KeyCode::Right) => return self.follow_downstream(),
            // In the trees, moving "outward" focuses the item; moving back returns to the node.
            (Pane::Up, KeyCode::Char('h') | KeyCode::Left | KeyCode::Enter)
            | (Pane::Down, KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter) => {
                let target = self
                    .current_item()
                    .filter(|i| !i.missing)
                    .map(|i| i.node.clone());
                return target.and_then(|id| self.focus_on(&id));
            }
            (Pane::Up, KeyCode::Char('l') | KeyCode::Right)
            | (Pane::Down, KeyCode::Char('h') | KeyCode::Left) => self.pane = Pane::Node,
            (pane @ (Pane::Up | Pane::Down), KeyCode::Char(' ')) => self.toggle_collapse(pane),

            (_, KeyCode::Char('p')) => return self.probe(self.probe_target()),
            (_, KeyCode::Char('P')) => {
                self.probe_follow = !self.probe_follow;
                if self.probe_follow {
                    return self.follow_probe();
                }
                self.probe = None;
                return Some(Action::ProbeOff);
            }
            _ => {}
        }
        None
    }

    fn toggle_collapse(&mut self, pane: Pane) {
        let Some(node) = self.current_item().map(|i| i.node.clone()) else {
            return;
        };
        let set = &mut self.collapsed[if pane == Pane::Up { 0 } else { 1 }];
        if !set.remove(&node) {
            set.insert(node);
        }
        self.set_cursor(pane, self.cursor(pane));
    }
}

/// How much a nudge key changes a number: `+`/`-` 1, `>`/`<` 10, `}`/`{` 50
/// (`=` and `_` also work, so ±1 needs no shift on US layouts).
fn nudge_step(c: char) -> Option<i64> {
    match c {
        '+' | '=' => Some(1),
        '-' | '_' => Some(-1),
        '>' => Some(10),
        '<' => Some(-10),
        '}' => Some(50),
        '{' => Some(-50),
        _ => None,
    }
}

/// The output a row stands for: an output, or a palette color.
fn row_output(row: &Row) -> Option<&str> {
    match row {
        Row::Output { port } | Row::Entry { key: port, .. } => Some(port),
        _ => None,
    }
}

/// What feeds a row: an input's source or a linked param's link.
fn row_source(row: Option<&Row>) -> Option<&Reference> {
    match row? {
        Row::Input {
            source: Some(s), ..
        }
        | Row::Param { link: Some(s), .. } => Some(s),
        _ => None,
    }
}

/// Pops history entries until one names a node that exists.
fn pop_existing(history: &mut Vec<String>, project: &Project) -> Option<String> {
    while let Some(id) = history.pop() {
        if project.nodes.contains_key(&id) {
            return Some(id);
        }
    }
    None
}

/// Inputs (one row per connection), params, outputs, then errors.
fn node_rows(project: &Project, node: &ProjectNode) -> Vec<Row> {
    let mut rows = Vec::new();
    for (port, refs) in node.input_ports() {
        let spec = node.spec.as_ref().and_then(|s| s.input(&port));
        if spec.is_some_and(|p| p.param) {
            continue; // shown on the param's row
        }
        let multi = spec.is_some_and(|p| p.multi);
        if refs.is_empty() {
            rows.push(Row::Input {
                port,
                index: None,
                source: None,
            });
            continue;
        }
        for (i, source) in refs.into_iter().enumerate() {
            let index = multi.then_some(i);
            rows.push(Row::Input {
                port: port.clone(),
                index,
                source: Some(source),
            });
        }
    }
    let spec_params = node
        .spec
        .iter()
        .flat_map(|s| &s.params)
        .filter(|p| p.kind != ParamKind::Colors)
        .map(|p| p.name.to_string());
    let extra_params = node
        .params
        .keys()
        .filter(|k| node.spec.as_ref().is_none_or(|s| s.param(k).is_none()))
        .cloned();
    rows.extend(spec_params.chain(extra_params).map(|name| Row::Param {
        link: node.inputs.get(&name).and_then(|r| r.first()).cloned(),
        name,
    }));
    // Palette colors: one row each, which also stands for the output it provides.
    for p in node.spec.iter().flat_map(|s| &s.params) {
        if p.kind != ParamKind::Colors {
            continue;
        }
        let keys = node.params.get(p.name).and_then(|v| v.as_table());
        rows.extend(
            keys.into_iter()
                .flat_map(|t| t.keys())
                .map(|key| Row::Entry {
                    param: p.name.to_string(),
                    key: key.clone(),
                }),
        );
    }
    rows.extend(
        node.spec
            .iter()
            .flat_map(|s| &s.outputs)
            .filter(|o| !o.entries)
            .map(|o| Row::Output {
                port: o.name.to_string(),
            }),
    );
    let prefix = format!("{}: ", node.path.display());
    rows.extend(project.errors_for(&node.id).into_iter().map(|e| {
        let text = e.to_string();
        Row::Error(text.strip_prefix(&prefix).unwrap_or(&text).to_string())
    }));
    rows
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use conduit_core::inspect;

    use super::*;

    fn example() -> App {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/basic");
        let registry = Arc::new(conduit_nodes::registry());
        let project = inspect(&dir, &registry).unwrap();
        App::new(dir, project, registry)
    }

    fn press(app: &mut App, keys: &str) -> Option<Action> {
        let mut last = None;
        for c in keys.chars() {
            last = app.handle_key(KeyEvent::from(KeyCode::Char(c)));
        }
        last
    }

    fn key(app: &mut App, code: KeyCode) -> Option<Action> {
        app.handle_key(KeyEvent::from(code))
    }

    fn focus(app: &App) -> &str {
        app.focus.as_deref().unwrap()
    }

    fn reference(s: &str) -> Reference {
        Reference::parse(s).unwrap()
    }

    #[test]
    fn starts_on_the_preview() {
        let app = example();
        assert_eq!(focus(&app), "preview");
        assert_eq!(
            app.rows,
            [
                Row::Input {
                    port: "image".into(),
                    index: None,
                    source: Some(reference("comp"))
                },
                Row::Param {
                    name: "title".into(),
                    link: None,
                },
            ]
        );
        let up: Vec<&str> = app.up.iter().map(|i| i.node.as_str()).collect();
        assert_eq!(up, ["comp", "white_bg", "placed", "black_sq"]);
        assert!(app.down.is_empty());
    }

    #[test]
    fn follow_flows_with_h_and_l() {
        let mut app = example();
        press(&mut app, "h");
        assert_eq!(focus(&app), "comp");
        assert_eq!(
            app.rows[app.cursor(Pane::Node)],
            Row::Output { port: "out".into() }
        );

        press(&mut app, "k"); // onto `layers[0] ← placed`
        press(&mut app, "h");
        assert_eq!(focus(&app), "placed");
        press(&mut app, "h");
        assert_eq!(focus(&app), "black_sq");
        press(&mut app, "h");
        assert_eq!(app.message.as_deref(), Some("no connected inputs"));

        // Downstream lands on the input we came through.
        press(&mut app, "l");
        assert_eq!(focus(&app), "placed");
        press(&mut app, "l");
        assert_eq!(focus(&app), "comp");
        assert_eq!(
            app.rows[app.cursor(Pane::Node)],
            Row::Input {
                port: "layers".into(),
                index: Some(0),
                source: Some(reference("placed"))
            }
        );
    }

    #[test]
    fn history() {
        let mut app = example();
        press(&mut app, "hh"); // preview → comp → white_bg (first input)
        assert_eq!(focus(&app), "white_bg");
        press(&mut app, "[");
        assert_eq!(focus(&app), "comp");
        press(&mut app, "[");
        assert_eq!(focus(&app), "preview");
        assert_eq!(press(&mut app, "["), None);
        press(&mut app, "]]");
        assert_eq!(focus(&app), "white_bg");
    }

    #[test]
    fn trees_navigate_and_collapse() {
        let mut app = example();
        press(&mut app, "H");
        assert_eq!(app.pane, Pane::Up);
        press(&mut app, " "); // collapse `comp`
        assert_eq!(app.visible(Pane::Up), [0]);
        press(&mut app, " ");
        assert_eq!(app.visible(Pane::Up).len(), 4);
        press(&mut app, "jj"); // placed
        key(&mut app, KeyCode::Enter);
        assert_eq!(focus(&app), "placed");
        assert_eq!(app.pane, Pane::Node);
        let down: Vec<&str> = app.down.iter().map(|i| i.node.as_str()).collect();
        assert_eq!(down, ["comp", "preview"]);
    }

    #[test]
    fn find_jumps_to_node() {
        let mut app = example();
        press(&mut app, "/wbg");
        assert_eq!(app.pick_matches()[0].0, "white_bg");
        key(&mut app, KeyCode::Enter);
        assert_eq!(focus(&app), "white_bg");
        assert_eq!(app.mode, Mode::Normal);

        press(&mut app, "/zzz");
        assert!(app.pick_matches().is_empty());
        key(&mut app, KeyCode::Enter);
        assert_eq!(focus(&app), "white_bg");
    }

    #[test]
    fn probe_targets() {
        let mut app = example();
        // On the preview's input row: probe its source.
        assert_eq!(press(&mut app, "p"), Some(Action::Probe(reference("comp"))));
        press(&mut app, "h");
        assert_eq!(
            press(&mut app, "p"),
            Some(Action::Probe(reference("comp.out")))
        );

        // Follow mode probes each newly focused node.
        assert_eq!(press(&mut app, "P"), Some(Action::Probe(reference("comp"))));
        assert_eq!(
            press(&mut app, "kh"),
            Some(Action::Probe(reference("placed")))
        );
        assert_eq!(press(&mut app, "P"), Some(Action::ProbeOff));
        assert_eq!(press(&mut app, "h"), None);
    }

    #[test]
    fn reload_keeps_or_resets_focus() {
        let mut app = example();
        press(&mut app, "hh");
        assert_eq!(focus(&app), "white_bg");
        let mut project = inspect(&app.dir, &conduit_nodes::registry()).unwrap();
        project.nodes.remove("white_bg");
        app.reload(project);
        assert_eq!(
            focus(&app),
            "preview",
            "missing focus falls back to the default"
        );
        press(&mut app, "[");
        assert_eq!(focus(&app), "comp", "history skips removed nodes");
    }

    #[test]
    fn broken_nodes_show_errors() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("a.toml"),
            "type = \"rect\"\n[params]\ncolor = \"nope\"\nwidth = 1\nheight = 1\n",
        )
        .unwrap();
        let registry = Arc::new(conduit_nodes::registry());
        let project = inspect(tmp.path(), &registry).unwrap();
        let app = App::new(tmp.path().to_path_buf(), project, registry);
        assert_eq!(
            app.rows.last(),
            Some(&Row::Error("params: unknown color `nope`".into()))
        );
    }

    // --- editing ---------------------------------------------------------------------

    /// The basic example copied to a temp folder, so edits can change it.
    fn temp_example() -> (tempfile::TempDir, App) {
        temp_project("basic")
    }

    /// An example project copied to a temp folder.
    fn temp_project(name: &str) -> (tempfile::TempDir, App) {
        let tmp = tempfile::tempdir().unwrap();
        let example = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples")
            .join(name);
        for entry in std::fs::read_dir(example).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "toml") {
                std::fs::copy(&path, tmp.path().join(path.file_name().unwrap())).unwrap();
            }
        }
        let registry = Arc::new(conduit_nodes::registry());
        let project = inspect(tmp.path(), &registry).unwrap();
        let app = App::new(tmp.path().to_path_buf(), project, registry);
        (tmp, app)
    }

    fn read(app: &App, id: &str) -> String {
        std::fs::read_to_string(app.dir.join(format!("{id}.toml"))).unwrap_or_default()
    }

    fn goto(app: &mut App, id: &str) {
        press(app, &format!("/{id}"));
        key(app, KeyCode::Enter);
        assert_eq!(focus(app), id);
    }

    fn select(app: &mut App, pred: impl Fn(&Row) -> bool) {
        let i = app.rows.iter().position(pred).expect("row exists");
        app.set_cursor(Pane::Node, i);
    }

    fn param(name: &str) -> impl Fn(&Row) -> bool + '_ {
        move |r| matches!(r, Row::Param { name: n, .. } if n == name)
    }

    fn input(port: &str, index: Option<usize>) -> impl Fn(&Row) -> bool + '_ {
        move |r| matches!(r, Row::Input { port: p, index: i, .. } if p == port && *i == index)
    }

    fn clear(app: &mut App) {
        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    }

    fn prompt_error(app: &App) -> Option<&str> {
        match &app.mode {
            Mode::Prompt { error, .. } => error.as_deref(),
            _ => None,
        }
    }

    #[test]
    fn edit_param_validate_reset_and_undo() {
        let (_tmp, mut app) = temp_example();
        let original = read(&app, "placed");
        goto(&mut app, "placed");
        select(&mut app, param("x"));

        key(&mut app, KeyCode::Enter);
        assert!(matches!(&app.mode, Mode::Prompt { input, .. } if input == "300"));
        clear(&mut app);
        press(&mut app, "120");
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.mode, Mode::Normal);
        assert!(read(&app, "placed").contains("x = 120"));
        assert_eq!(app.message.as_deref(), Some("set placed.x = 120"));

        // Invalid input keeps the prompt open and changes nothing.
        key(&mut app, KeyCode::Enter);
        clear(&mut app);
        press(&mut app, "abc");
        key(&mut app, KeyCode::Enter);
        assert!(prompt_error(&app).unwrap().contains("not a whole number"));
        key(&mut app, KeyCode::Esc);
        assert!(read(&app, "placed").contains("x = 120"));

        // Empty input resets to the default.
        key(&mut app, KeyCode::Enter);
        clear(&mut app);
        key(&mut app, KeyCode::Enter);
        assert!(!read(&app, "placed").contains("x ="));

        press(&mut app, "uu");
        assert_eq!(read(&app, "placed"), original);
        assert_eq!(app.message.as_deref(), Some("undid: set placed.x = 120"));
        press(&mut app, "u");
        assert_eq!(app.message.as_deref(), Some("nothing to undo"));
    }

    #[test]
    fn param_range_is_checked() {
        let (_tmp, mut app) = temp_example();
        goto(&mut app, "black_sq");
        select(&mut app, param("width"));
        key(&mut app, KeyCode::Enter);
        clear(&mut app);
        press(&mut app, "0");
        key(&mut app, KeyCode::Enter);
        assert_eq!(prompt_error(&app), Some("must be between 1 and 16384"));
    }

    #[test]
    fn connect_and_disconnect() {
        let (_tmp, mut app) = temp_example();
        goto(&mut app, "comp");
        select(&mut app, input("layers", Some(0)));
        press(&mut app, "c");
        let keys: Vec<String> = app.pick_matches().into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys.len(), 3, "{keys:?}"); // black_sq, placed, white_bg: not comp or preview
        press(&mut app, "white");
        key(&mut app, KeyCode::Enter);
        assert!(
            read(&app, "comp").contains("layers = [\"placed\", \"white_bg\"]"),
            "{}",
            read(&app, "comp")
        );
        assert_eq!(
            app.message.as_deref(),
            Some("connected comp.layers ← white_bg")
        );

        select(&mut app, input("layers", Some(1)));
        press(&mut app, "d");
        assert!(read(&app, "comp").contains("layers = [\"placed\"]"));

        // A single input is replaced.
        select(&mut app, input("base", None));
        press(&mut app, "cblack");
        key(&mut app, KeyCode::Enter);
        assert!(read(&app, "comp").contains("base = \"black_sq\""));
        assert!(app.project.is_valid());
    }

    #[test]
    fn add_node() {
        let (_tmp, mut app) = temp_example();
        press(&mut app, "arect");
        key(&mut app, KeyCode::Enter);
        assert!(matches!(&app.mode, Mode::Prompt { input, .. } if input == "rect_1"));
        clear(&mut app);
        press(&mut app, "bad id");
        key(&mut app, KeyCode::Enter);
        assert!(prompt_error(&app).unwrap().contains("not a valid node id"));
        clear(&mut app);
        press(&mut app, "red_box");
        key(&mut app, KeyCode::Enter);
        assert_eq!(focus(&app), "red_box");
        assert!(read(&app, "red_box").starts_with("type = \"rect\""));
        assert!(app.project.is_valid());
    }

    fn link(app: &App, name: &str) -> Option<Reference> {
        app.rows.iter().find_map(|r| match r {
            Row::Param { name: n, link } if n == name => link.clone(),
            _ => None,
        })
    }

    #[test]
    fn link_number_params() {
        let (_tmp, mut app) = temp_example();
        let original = read(&app, "black_sq");
        goto(&mut app, "black_sq");

        // `i` on a number param: feed it from a new int that starts at its value.
        select(&mut app, param("width"));
        press(&mut app, "i");
        let kinds: Vec<String> = app.pick_matches().into_iter().map(|(k, _)| k).collect();
        assert_eq!(kinds, ["int"]);
        key(&mut app, KeyCode::Enter);
        assert!(matches!(&app.mode, Mode::Prompt { input, .. } if input == "black_sq_width"));
        key(&mut app, KeyCode::Enter);
        assert_eq!(focus(&app), "black_sq", "focus stays on the fed node");
        assert!(read(&app, "black_sq_width").contains("value = 50"));
        assert_eq!(link(&app, "width"), Some(reference("black_sq_width")));
        assert!(app.project.is_valid(), "{:?}", app.project.errors);
        assert!(
            !app.rows.iter().any(input("width", None)),
            "param inputs aren't shown as input rows"
        );

        // `c` links another number param to an existing number output; only numbers fit.
        select(&mut app, param("height"));
        press(&mut app, "c");
        let keys: Vec<String> = app.pick_matches().into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["black_sq_width.out"]);
        key(&mut app, KeyCode::Enter);
        assert_eq!(link(&app, "height"), Some(reference("black_sq_width")));

        // `h` follows the link; `l` from there comes back to the param row.
        press(&mut app, "h");
        assert_eq!(focus(&app), "black_sq_width");
        press(&mut app, "l");
        assert_eq!(focus(&app), "black_sq");
        assert!(matches!(app.current_row(), Some(Row::Param { .. })));

        // `d` unlinks; typing a value replaces a link.
        select(&mut app, param("height"));
        press(&mut app, "d");
        assert_eq!(link(&app, "height"), None);
        assert_eq!(
            app.message.as_deref(),
            Some("unlinked black_sq.height from black_sq_width")
        );
        select(&mut app, param("width"));
        key(&mut app, KeyCode::Enter);
        assert!(matches!(&app.mode, Mode::Prompt { input, .. } if input == "50"));
        clear(&mut app);
        press(&mut app, "70");
        key(&mut app, KeyCode::Enter);
        assert_eq!(link(&app, "width"), None);
        assert!(read(&app, "black_sq").contains("width = 70"));

        // Colors can be linked too, but this project has no color outputs to offer.
        select(&mut app, param("color"));
        press(&mut app, "c");
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(
            app.message.as_deref(),
            Some("no compatible outputs to connect")
        );

        press(&mut app, "uuuu");
        assert_eq!(read(&app, "black_sq"), original);
        assert_eq!(read(&app, "black_sq_width"), "", "new node removed by undo");
    }

    fn int_param(app: &App, id: &str, name: &str) -> Option<i64> {
        app.project.get(id)?.params.get(name)?.as_integer()
    }

    #[test]
    fn nudge_whole_numbers() {
        let (_tmp, mut app) = temp_example();
        let original = read(&app, "placed");
        goto(&mut app, "placed");
        select(&mut app, param("x"));
        press(&mut app, "+>}-=_");
        assert_eq!(
            int_param(&app, "placed", "x"),
            Some(300 + 1 + 10 + 50 - 1 + 1 - 1)
        );
        assert_eq!(app.message.as_deref(), Some("nudged placed.x to 360 (-1)"));
        // Optional params start from their default; position.x can go negative.
        select(&mut app, param("y"));
        press(&mut app, "{{{{{{{");
        assert_eq!(int_param(&app, "placed", "y"), Some(300 - 350));

        // Each param's run of nudges undoes in one step.
        press(&mut app, "u");
        assert_eq!(int_param(&app, "placed", "y"), Some(300));
        assert_eq!(int_param(&app, "placed", "x"), Some(360));
        press(&mut app, "u");
        assert_eq!(read(&app, "placed"), original);

        // Clamped to the range.
        goto(&mut app, "black_sq");
        select(&mut app, param("width"));
        press(&mut app, "{");
        assert_eq!(int_param(&app, "black_sq", "width"), Some(1));
        press(&mut app, "-");
        assert_eq!(
            app.message.as_deref(),
            Some("black_sq.width is at its minimum, 1")
        );

        // Colors are adjusted in the color editor instead.
        select(&mut app, param("color"));
        press(&mut app, "+");
        assert_eq!(
            app.message.as_deref(),
            Some("press ⏎ to adjust `color` in the color editor")
        );
    }

    #[test]
    fn nudge_through_a_link() {
        let (_tmp, mut app) = temp_example();
        goto(&mut app, "black_sq");
        select(&mut app, param("height"));
        press(&mut app, "i");
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Enter); // int `black_sq_height`, value 50
        select(&mut app, param("height"));
        press(&mut app, "}>");
        assert_eq!(int_param(&app, "black_sq_height", "value"), Some(110));
        assert_eq!(focus(&app), "black_sq");
        assert_eq!(link(&app, "height"), Some(reference("black_sq_height")));
    }

    fn entry(key: &str) -> impl Fn(&Row) -> bool + '_ {
        move |r| matches!(r, Row::Entry { key: k, .. } if k == key)
    }

    fn entry_keys(app: &App) -> Vec<String> {
        app.rows
            .iter()
            .filter_map(|r| match r {
                Row::Entry { key, .. } => Some(key.clone()),
                _ => None,
            })
            .collect()
    }

    fn color_edit(app: &App) -> &ColorEdit {
        match &app.mode {
            Mode::Color(edit) => edit,
            other => panic!("expected the color editor, got {other:?}"),
        }
    }

    #[test]
    fn palette_rows() {
        let (_tmp, mut app) = temp_project("palette");
        let original = read(&app, "brand");
        goto(&mut app, "brand");
        assert_eq!(
            entry_keys(&app),
            ["primary", "teal", "sand", "orange", "accent"]
        );
        assert!(
            !app.rows.iter().any(|r| matches!(r, Row::Output { .. })
                || matches!(r, Row::Param { name, .. } if name == "colors")),
            "colors are shown as entries, not as a param or outputs: {:?}",
            app.rows
        );

        // `l` follows a color to what uses it; `h` comes back to that color.
        select(&mut app, entry("accent"));
        press(&mut app, "l");
        assert_eq!(focus(&app), "accent_dark");
        press(&mut app, "h");
        assert_eq!(focus(&app), "brand");
        assert!(matches!(app.current_row(), Some(Row::Entry { key, .. }) if key == "accent"));

        // Rename: references follow.
        press(&mut app, "r");
        clear(&mut app);
        press(&mut app, "coral");
        key(&mut app, KeyCode::Enter);
        assert!(read(&app, "accent_dark").contains("color = \"brand.coral\""));
        assert!(app.project.is_valid(), "{:?}", app.project.errors);

        // Add a color (a copy of the one under the cursor), then remove it.
        press(&mut app, "o");
        key(&mut app, KeyCode::Enter); // accept `c1`
        assert!(read(&app, "brand").contains("c1 = \"#e76f51\""));
        assert!(matches!(app.current_row(), Some(Row::Entry { key, .. }) if key == "c1"));
        press(&mut app, "x");
        assert!(
            !read(&app, "brand").contains("c1 ="),
            "unused: removed at once"
        );

        // Removing a used color asks first and disconnects its users.
        select(&mut app, entry("primary"));
        press(&mut app, "x");
        assert!(
            matches!(&app.mode, Mode::Confirm { question, .. } if question.contains("stripe.color"))
        );
        press(&mut app, "y");
        assert!(!read(&app, "stripe").contains("brand.primary"));

        press(&mut app, "uuuu");
        assert_eq!(read(&app, "brand"), original);
        assert!(read(&app, "stripe").contains("brand.primary"));
    }

    #[test]
    fn import_into_palette() {
        let (_tmp, mut app) = temp_project("palette");
        goto(&mut app, "brand");
        press(&mut app, "I");
        press(&mut app, "https://coolors.co/000000-ffffff");
        key(&mut app, KeyCode::Enter);
        assert_eq!(entry_keys(&app), ["c1", "c2"]);
        assert!(read(&app, "brand").contains("source = \"https://coolors.co/000000-ffffff\""));
        // The old names are gone, so their users are now broken until relinked.
        assert!(!app.project.is_valid());
        press(&mut app, "u");
        assert_eq!(entry_keys(&app).len(), 5);

        // Adding a palette goes straight to the import prompt.
        press(&mut app, "apalette");
        key(&mut app, KeyCode::Enter);
        clear(&mut app);
        press(&mut app, "greys");
        key(&mut app, KeyCode::Enter);
        assert!(matches!(
            &app.mode,
            Mode::Prompt {
                purpose: Prompt::Import { .. },
                ..
            }
        ));
        press(&mut app, "--light: #eeeeee; --dark: #222222;");
        key(&mut app, KeyCode::Enter);
        assert_eq!(focus(&app), "greys");
        assert_eq!(entry_keys(&app), ["light", "dark"]);
    }

    #[test]
    fn color_editor() {
        let (_tmp, mut app) = temp_project("palette");
        let original = read(&app, "brand");
        goto(&mut app, "brand");
        select(&mut app, entry("accent"));

        key(&mut app, KeyCode::Enter);
        assert_eq!(conduit_core::to_hex(color_edit(&app).color), "#e76f51");
        // Saturation down 50 points, written live; esc puts everything back.
        press(&mut app, "j{");
        assert_ne!(read(&app, "brand"), original);
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(read(&app, "brand"), original);

        // RGB: red down 50; a typed hex; ⏎ keeps. One undo step for the whole session.
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Tab);
        press(&mut app, "{");
        assert!(
            read(&app, "brand").contains("accent = \"#b56f51\""),
            "{}",
            read(&app, "brand")
        );
        press(&mut app, "#00ff00");
        key(&mut app, KeyCode::Enter);
        assert!(read(&app, "brand").contains("accent = \"#00ff00\""));
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.mode, Mode::Normal);
        press(&mut app, "u");
        assert_eq!(read(&app, "brand"), original);

        // On a linked color param, typing replaces the link.
        goto(&mut app, "square");
        select(&mut app, param("color"));
        key(&mut app, KeyCode::Enter);
        press(&mut app, "#123456");
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Enter);
        assert_eq!(link(&app, "color"), None);
        assert!(read(&app, "square").contains("color = \"#123456\""));
    }

    #[test]
    fn feed_a_color_param_from_a_color_node() {
        let (_tmp, mut app) = temp_example();
        goto(&mut app, "black_sq");
        select(&mut app, param("color"));
        press(&mut app, "i");
        let kinds: Vec<String> = app.pick_matches().into_iter().map(|(k, _)| k).collect();
        assert_eq!(kinds, ["color"], "palettes only have named outputs");
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Enter); // `black_sq_color`
        assert!(read(&app, "black_sq_color").contains("color = \"black\""));
        assert_eq!(link(&app, "color"), Some(reference("black_sq_color")));
        assert!(app.project.is_valid(), "{:?}", app.project.errors);
    }

    #[test]
    fn insert_node_into_connection() {
        let (_tmp, mut app) = temp_example();
        goto(&mut app, "comp");
        select(&mut app, input("base", None));
        press(&mut app, "i");
        let kinds: Vec<String> = app.pick_matches().into_iter().map(|(k, _)| k).collect();
        assert_eq!(kinds, ["composite"], "image → image: only composite fits");
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Enter); // accept `composite_1`
        assert_eq!(focus(&app), "composite_1");
        assert!(read(&app, "comp").contains("base = \"composite_1\""));
        assert!(read(&app, "composite_1").contains("base = \"white_bg\""));
        assert!(app.project.is_valid(), "{:?}", app.project.errors);
    }

    #[test]
    fn rename_node() {
        let (_tmp, mut app) = temp_example();
        goto(&mut app, "comp");
        goto(&mut app, "white_bg");
        press(&mut app, "r");
        clear(&mut app);
        press(&mut app, "bg");
        key(&mut app, KeyCode::Enter);
        assert_eq!(focus(&app), "bg");
        assert!(read(&app, "white_bg").is_empty());
        assert!(read(&app, "comp").contains("base = \"bg\""));
        press(&mut app, "[");
        assert_eq!(focus(&app), "comp");
        press(&mut app, "]");
        assert_eq!(focus(&app), "bg", "history follows the rename");
    }

    #[test]
    fn delete_node_with_confirmation() {
        let (_tmp, mut app) = temp_example();
        goto(&mut app, "comp");
        goto(&mut app, "placed");
        press(&mut app, "Dn");
        assert!(!read(&app, "placed").is_empty());
        assert!(app.message.as_deref().unwrap().starts_with("cancelled"));

        press(&mut app, "D");
        assert!(
            matches!(&app.mode, Mode::Confirm { question, .. } if question.contains("disconnects comp.layers"))
        );
        press(&mut app, "y");
        assert!(read(&app, "placed").is_empty());
        assert!(
            !read(&app, "comp").contains("layers"),
            "emptied list removed"
        );
        assert_eq!(focus(&app), "comp", "focus returns to the previous node");

        press(&mut app, "u");
        assert!(!read(&app, "placed").is_empty());
        assert!(read(&app, "comp").contains("layers = [\"placed\"]"));
    }

    #[test]
    fn open_in_editor() {
        let (_tmp, mut app) = temp_example();
        let path = app.focused().unwrap().path.clone();
        assert_eq!(press(&mut app, "E"), Some(Action::OpenEditor(path)));
    }
}
