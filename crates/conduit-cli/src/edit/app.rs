//! Editor state and key handling, independent of the terminal (see `view.rs` for drawing).

use std::collections::BTreeSet;
use std::path::PathBuf;

use conduit_core::{Project, ProjectNode, Reference, TreeItem};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

use super::finder;
use crate::run::Event;

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
    Param {
        name: String,
    },
    Output {
        port: String,
    },
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Normal,
    Find { query: String, selected: usize },
    Help,
}

/// What the event loop should do after a key.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Quit,
    Probe(Reference),
    ProbeOff,
}

pub struct App {
    pub dir: PathBuf,
    pub project: Project,
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
    pub fn new(dir: PathBuf, project: Project) -> Self {
        let mut app = Self {
            dir,
            project,
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
    pub fn reload(&mut self, project: Project) {
        self.project = project;
        let exists = |id: &String| self.project.nodes.contains_key(id);
        self.back.retain(exists);
        self.forward.retain(exists);
        if !self.focus.as_ref().is_some_and(exists) {
            self.focus = self.default_focus();
        }
        self.refresh();
    }

    pub fn on_runner_event(&mut self, event: Event) {
        if event != Event::FilesChanged {
            self.status = Some(event);
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
        let id = self.back.pop()?;
        if let Some(cur) = self.focus.take() {
            self.forward.push(cur);
        }
        self.set_focus(&id)
    }

    fn go_forward(&mut self) -> Option<Action> {
        let id = self.forward.pop()?;
        if let Some(cur) = self.focus.take() {
            self.back.push(cur);
        }
        self.set_focus(&id)
    }

    /// `h` in the node panel: go to the source of the input under the cursor (or the first
    /// connected input), landing on the output we came through.
    fn follow_upstream(&mut self) -> Option<Action> {
        let source = match self.current_row() {
            Some(Row::Input {
                source: Some(s), ..
            }) => Some(s.clone()),
            _ => self.rows.iter().find_map(|r| match r {
                Row::Input {
                    source: Some(s), ..
                } => Some(s.clone()),
                _ => None,
            }),
        };
        let Some(source) = source else {
            self.message = Some("no connected inputs".into());
            return None;
        };
        let action = self.focus_on(&source.node);
        self.select_row(|r| matches!(r, Row::Output { port } if *port == source.port));
        action
    }

    /// `l` in the node panel: go to the first consumer of the output under the cursor (or
    /// of any output), landing on the input we came through.
    fn follow_downstream(&mut self) -> Option<Action> {
        let id = self.focus.clone()?;
        let consumers = self.project.consumers(&id);
        let consumer = match self.current_row() {
            Some(Row::Output { port }) => consumers.iter().find(|c| c.output == *port),
            _ => None,
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
        self.select_row(
            |r| matches!(r, Row::Input { port, index: i, .. } if *port == c.port && *i == index),
        );
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
        match self.current_row() {
            Some(Row::Input {
                source: Some(s), ..
            }) => Some(s.clone()),
            Some(Row::Output { port }) => Some(Reference {
                node: self.focus.clone()?,
                port: port.clone(),
            }),
            _ => self.default_probe(self.focus.as_deref()?),
        }
    }

    fn default_probe(&self, id: &str) -> Option<Reference> {
        let node = self.project.get(id)?;
        if let Some(out) = node.spec.as_ref().and_then(|s| s.outputs.first()) {
            return Some(Reference {
                node: id.to_string(),
                port: out.name.to_string(),
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

    /// Node ids matching the find query, best first.
    pub fn find_matches(&self) -> Vec<String> {
        let Mode::Find { query, .. } = &self.mode else {
            return Vec::new();
        };
        let haystacks: Vec<(String, String)> = self
            .project
            .nodes
            .values()
            .map(|n| {
                (
                    n.id.clone(),
                    format!("{} {}", n.id, n.kind.as_deref().unwrap_or("")),
                )
            })
            .collect();
        finder::rank(
            query,
            haystacks
                .iter()
                .map(|(id, text)| (id.clone(), text.as_str())),
        )
    }

    // --- keys --------------------------------------------------------------------------

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        self.message = None;
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Some(Action::Quit);
        }
        match self.mode.clone() {
            Mode::Help => {
                self.mode = Mode::Normal;
                None
            }
            Mode::Find { query, selected } => self.find_key(key, query, selected),
            Mode::Normal => self.normal_key(key),
        }
    }

    fn find_key(
        &mut self,
        key: KeyEvent,
        mut query: String,
        mut selected: usize,
    ) -> Option<Action> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                return None;
            }
            KeyCode::Enter => {
                let id = self.find_matches().into_iter().nth(selected);
                self.mode = Mode::Normal;
                return id.and_then(|id| self.focus_on(&id));
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
        self.mode = Mode::Find { query, selected };
        let count = self.find_matches().len();
        if let Mode::Find { selected, .. } = &mut self.mode {
            *selected = (*selected).min(count.saturating_sub(1));
        }
        None
    }

    fn normal_key(&mut self, key: KeyEvent) -> Option<Action> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match (self.pane, key.code) {
            (_, KeyCode::Char('q')) => return Some(Action::Quit),
            (_, KeyCode::Char('?')) => self.mode = Mode::Help,
            (_, KeyCode::Char('/')) => {
                self.mode = Mode::Find {
                    query: String::new(),
                    selected: 0,
                }
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

/// Inputs (one row per connection), params, outputs, then errors.
fn node_rows(project: &Project, node: &ProjectNode) -> Vec<Row> {
    let mut rows = Vec::new();
    for (port, refs) in node.input_ports() {
        let multi = node
            .spec
            .as_ref()
            .and_then(|s| s.input(&port))
            .is_some_and(|p| p.multi);
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
        .map(|p| p.name.to_string());
    let extra_params = node
        .params
        .keys()
        .filter(|k| node.spec.as_ref().is_none_or(|s| s.param(k).is_none()))
        .cloned();
    rows.extend(
        spec_params
            .chain(extra_params)
            .map(|name| Row::Param { name }),
    );
    rows.extend(
        node.spec
            .iter()
            .flat_map(|s| &s.outputs)
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
        let project = inspect(&dir, &conduit_nodes::registry()).unwrap();
        App::new(dir, project)
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
                    name: "title".into()
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
        assert_eq!(app.find_matches()[0], "white_bg");
        key(&mut app, KeyCode::Enter);
        assert_eq!(focus(&app), "white_bg");
        assert_eq!(app.mode, Mode::Normal);

        press(&mut app, "/zzz");
        assert!(app.find_matches().is_empty());
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
        let project = inspect(tmp.path(), &conduit_nodes::registry()).unwrap();
        let app = App::new(tmp.path().to_path_buf(), project);
        assert_eq!(
            app.rows.last(),
            Some(&Row::Error("params: unknown color `nope`".into()))
        );
    }
}
