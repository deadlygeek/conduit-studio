//! Editor state and key handling, independent of the terminal (see `view.rs` for drawing).

mod colors;
mod edit_view;
mod edits;
mod files;
mod flow_edits;
mod fonts;
mod live;
mod spotlight;
mod walk;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use conduit_core::{
    Change, ParamKind, ParamSpec, PortSpec, Project, ProjectNode, Reference, Registry, TreeItem,
};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

use crate::run::{Event, Shown};
pub use colors::ColorEdit;
pub use edit_view::{EditRow, shown};
pub use edits::{Confirm, Pick, Prompt, uses};
pub use walk::Bar;

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
    /// `selected`: the whole input is highlighted, so typing replaces it (as when a
    /// prompt opens on the current value).
    Prompt {
        purpose: Prompt,
        input: String,
        selected: bool,
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
    /// Launch the GUI color picker with these arguments (it runs alongside the TUI).
    OpenPicker(Vec<String>),
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
    /// Spotlight (`s`): what the focus draws is tinted in the preview.
    pub spotlight: bool,
    /// The spotlight targets last sent to the renderer.
    spotlight_sent: Vec<String>,
    /// Installed font families for the font picker, loaded when it first opens.
    fonts: Option<Arc<Vec<conduit_nodes::text::FontFamily>>>,
    /// Fonts chosen in this session, most recent last; listed first in the picker.
    recent_fonts: Vec<String>,
    /// The open file picker's choices (path, label), listed when it opens.
    files: Vec<(String, String)>,
    /// A node picked up with `m`, to be put into the next connection chosen.
    pub carrying: Option<String>,
    /// Show the flow view (spec `docs/specs/node-editor-ui.md`) instead of the three
    /// panes; `v` switches while the flow view is being built.
    pub flow_view: bool,
    /// The last frame was below the flow view's minimum size (§4.6): only `q` works.
    pub too_small: bool,
    /// The flow on screen and the focus on it; `focus` is kept on the focus's node.
    pub flow: super::flow::Flow,
    pub flow_focus: Option<super::flow::Focus>,
    /// The flow view's scroll position; `recentre` after a flow change (§4.5).
    pub viewport: super::canvas::Viewport,
    pub recentre: bool,
    /// A render finished since the last key: the status line shows its result.
    pub show_render: bool,
    /// A flow-view edit waiting for its popups to close (see `flow_edits.rs`).
    pending: Option<flow_edits::Pending>,
    /// Flow history (§6.1): the flow and focus before each flow change, and after `[`.
    flow_back: Vec<(super::flow::Flow, super::flow::Focus)>,
    flow_forward: Vec<(super::flow::Flow, super::flow::Focus)>,
    /// The edit view's cursor while it's open (§7).
    pub edit_view: Option<EditRow>,
    /// The cursor's position among the edit view's rows, for when its row goes.
    edit_at: usize,
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
            spotlight: false,
            spotlight_sent: Vec::new(),
            fonts: None,
            recent_fonts: Vec::new(),
            files: Vec::new(),
            carrying: None,
            flow_view: true,
            too_small: false,
            flow: Default::default(),
            flow_focus: None,
            viewport: Default::default(),
            recentre: true,
            show_render: false,
            pending: None,
            flow_back: Vec::new(),
            flow_forward: Vec::new(),
            edit_view: None,
            edit_at: 0,
        };
        app.focus = app.default_focus();
        app.refresh();
        app.sync_flow();
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

    /// The focused node's declared param `name`.
    pub(super) fn param_spec(&self, name: &str) -> Option<&ParamSpec> {
        self.focused()?.spec.as_ref()?.param(name)
    }

    /// The focused node's declared input `port`.
    pub(super) fn input_spec(&self, port: &str) -> Option<&PortSpec> {
        self.focused()?.spec.as_ref()?.input(port)
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
        self.sync_flow();
    }

    pub fn on_runner_event(&mut self, event: Event) {
        if event != Event::FilesChanged {
            self.show_render = true;
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

    /// A side pane's tree and its collapsed nodes; `None` for the node pane.
    fn tree(&self, pane: Pane) -> Option<(&[TreeItem], &BTreeSet<String>)> {
        match pane {
            Pane::Up => Some((&self.up, &self.collapsed[0])),
            Pane::Down => Some((&self.down, &self.collapsed[1])),
            Pane::Node => None,
        }
    }

    /// Indices into the pane's tree of the items not hidden by a collapsed ancestor.
    pub fn visible(&self, pane: Pane) -> Vec<usize> {
        let Some((items, collapsed)) = self.tree(pane) else {
            return (0..self.rows.len()).collect();
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
        self.tree(pane)
            .is_some_and(|(_, collapsed)| collapsed.contains(node))
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

    /// The current side pane's tree and the index of the item under its cursor.
    fn current_tree_index(&self) -> Option<(&[TreeItem], usize)> {
        let (tree, _) = self.tree(self.pane)?;
        let i = *self.visible(self.pane).get(self.cursor(self.pane))?;
        Some((tree, i))
    }

    /// The tree item under the cursor in a side pane.
    fn current_item(&self) -> Option<&TreeItem> {
        self.current_tree_index().map(|(tree, i)| &tree[i])
    }

    /// The node `D`, `X` and `m` act on: the highlighted row's in a tree, else the focus.
    fn target_node(&self) -> Option<String> {
        match self.pane {
            Pane::Node => self.focus.clone(),
            _ => self
                .current_item()
                .filter(|i| !i.missing)
                .map(|i| i.node.clone()),
        }
    }

    /// The connection a tree row stands for. In the inputs tree the row's node feeds its
    /// parent; in the used-by tree the parent feeds the row's node. The parent is the
    /// nearest row above at one less depth, or the focused node.
    fn tree_connection(&self) -> Option<conduit_core::edit::Connection> {
        let (tree, i) = self.current_tree_index()?;
        let item = &tree[i];
        let parent = tree[..i]
            .iter()
            .rev()
            .find(|p| p.depth < item.depth)
            .map(|p| p.node.clone())
            .or_else(|| self.focus.clone())?;
        let (source, node) = if self.pane == Pane::Up {
            (item.node.clone(), parent)
        } else {
            (parent, item.node.clone())
        };
        Some(conduit_core::edit::Connection {
            source: Reference {
                node: source,
                port: item.output.clone(),
            },
            node,
            port: item.port.clone(),
            index: item.index,
        })
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
        let multi = self.input_spec(&c.port).is_some_and(|p| p.multi);
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
        self.show_render = false;
        let action = self.handle_key_inner(key);
        if self.flow_view {
            self.finish_flow_edit();
            self.sync_flow();
            self.sync_edit_view();
        }
        action
    }

    fn handle_key_inner(&mut self, key: KeyEvent) -> Option<Action> {
        self.message = None;
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Some(Action::Quit);
        }
        if self.flow_view && self.too_small {
            return (key.code == KeyCode::Char('q')).then_some(Action::Quit);
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
                selected,
                error,
            } => self.prompt_key(key, purpose, input, selected, error),
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
        // Pickers that write the highlighted value as you move.
        let live = match &purpose {
            Pick::Font { param } | Pick::File { param } => Some(param.clone()),
            _ => None,
        };
        match key.code {
            KeyCode::Esc => {
                if let Some(param) = live {
                    self.revert_pick(&param);
                }
                return None;
            }
            KeyCode::Enter => {
                let chosen = self.matches_for(&purpose, &query).into_iter().nth(selected);
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
        let count = self.matches_for(&purpose, &query).len();
        self.mode = Mode::Pick {
            purpose,
            query,
            selected: selected.min(count.saturating_sub(1)),
        };
        if let Some(param) = live {
            self.preview_pick(&param);
        }
        None
    }

    /// Keys while a prompt is open.
    fn prompt_key(
        &mut self,
        key: KeyEvent,
        purpose: Prompt,
        mut input: String,
        selected: bool,
        mut error: Option<String>,
    ) -> Option<Action> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return None,
            KeyCode::Enter => match self.submit_prompt(purpose.clone(), &input) {
                Ok(action) => return action,
                Err(e) => error = Some(e),
            },
            // With the value highlighted, deleting or typing replaces all of it.
            KeyCode::Backspace | KeyCode::Delete if selected => input.clear(),
            KeyCode::Backspace => {
                input.pop();
            }
            KeyCode::Char('u') if ctrl => input.clear(),
            KeyCode::Char(c) if !ctrl => {
                if selected {
                    input.clear();
                }
                input.push(c);
            }
            // Any other key (e.g. → or End) keeps the value to edit it.
            _ => {}
        }
        self.mode = Mode::Prompt {
            purpose,
            input,
            selected: false,
            error,
        };
        None
    }

    fn normal_key(&mut self, key: KeyEvent) -> Option<Action> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.flow_view {
            if let Some(action) = self.edit_view_key(key) {
                return action;
            }
            if let Some(action) = self.flow_key(key) {
                return action;
            }
            if !Self::is_flow_global(&key)
                || self.carrying.is_some() && !Self::is_moving_global(&key)
            {
                return None;
            }
        }
        // Carrying a node (`m`): `⏎` drops it, `esc` cancels; everything else works as usual.
        if let Some(id) = self.carrying.clone().filter(|_| !self.flow_view) {
            match key.code {
                KeyCode::Enter => {
                    self.carrying = None;
                    self.drop_carried(id);
                    return None;
                }
                KeyCode::Esc => {
                    self.carrying = None;
                    self.message = Some(format!("stopped moving {id}"));
                    return None;
                }
                _ => {}
            }
        }
        match (self.pane, key.code) {
            (_, KeyCode::Char('q')) => return Some(Action::Quit),
            (_, KeyCode::Char('v')) => self.flow_view = !self.flow_view,
            (_, KeyCode::Char('?')) => self.mode = Mode::Help,
            (_, KeyCode::Char('/')) => self.open_pick(Pick::FindNode),

            // Editing.
            (Pane::Node, KeyCode::Enter) => self.edit_row(),
            (Pane::Node, KeyCode::Char('c')) => self.connect_row(),
            (Pane::Node, KeyCode::Char('d')) => self.disconnect_row(),
            (Pane::Node, KeyCode::Char('i')) => self.insert_row(),
            (_, KeyCode::Char('i')) => self.insert_tree_row(),
            (Pane::Node, KeyCode::Char('K')) => self.move_row(-1),
            (Pane::Node, KeyCode::Char('J')) => self.move_row(1),
            (Pane::Node, KeyCode::Char(c)) if nudge_step(c).is_some() => {
                self.nudge_row(nudge_step(c).expect("checked"))
            }
            (_, KeyCode::Char('a')) => self.open_pick(Pick::AddType),
            (Pane::Node, KeyCode::Char('o')) if !ctrl => self.start_add_entry(),
            (Pane::Node, KeyCode::Char('x')) => self.start_remove_entry(),
            (Pane::Node, KeyCode::Char('C')) => return self.open_picker(),
            (_, KeyCode::Char('I')) => match self.entries_param() {
                Some(param) => self.open_prompt(Prompt::Import { param }, String::new()),
                None => self.message = Some("I imports colors into a palette".into()),
            },
            (_, KeyCode::Char('r')) => self.start_rename(),
            (_, KeyCode::Char('D')) => self.start_delete(),
            (_, KeyCode::Char('X')) => self.start_dissolve(),
            (_, KeyCode::Char('m')) => self.start_move(),
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
            (_, KeyCode::Char('s')) => self.toggle_spotlight(),
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
        .filter(|p| p.kind != ParamKind::Colors && !node.param_hidden(p.name))
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
    // Every output except palette entries, which are listed above (repeated outputs such
    // as a `duplicate`'s `out1`, `out2` follow the node's params).
    if let Some(spec) = &node.spec {
        let mut params = node.params.clone();
        for o in spec.outputs.iter().filter(|o| o.entries) {
            params.remove(o.name);
        }
        rows.extend(
            spec.outputs_for(&params)
                .into_iter()
                .map(|(port, _)| Row::Output { port }),
        );
    }
    let prefix = format!("{}: ", node.path.display());
    rows.extend(project.errors_for(&node.id).into_iter().map(|e| {
        let text = e.to_string();
        Row::Error(text.strip_prefix(&prefix).unwrap_or(&text).to_string())
    }));
    rows
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;

    use conduit_core::inspect;

    use super::*;

    fn example() -> App {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/basic");
        let registry = Arc::new(conduit_nodes::registry());
        let project = inspect(&dir, &registry).unwrap();
        // These tests drive the three panes; the flow view's are in `walk.rs`.
        let mut app = App::new(dir, project, registry);
        app.flow_view = false;
        app
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
            "type = \"canvas\"\n[params]\ncolor = \"nope\"\nwidth = 1\nheight = 1\n",
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

    /// The keys of the picker's current matches.
    fn pick_keys(app: &App) -> Vec<String> {
        app.pick_matches().into_iter().map(|(k, _)| k).collect()
    }

    /// An example project copied to a temp folder.
    pub(crate) fn temp_project(name: &str) -> (tempfile::TempDir, App) {
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
        let mut app = App::new(tmp.path().to_path_buf(), project, registry);
        app.flow_view = false;
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
        assert!(prompt_error(&app).unwrap().contains("not a number"));
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
        goto(&mut app, "white_bg");
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
        let keys = pick_keys(&app);
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

        // Only images fit a base: a shape is a layer, not a background.
        select(&mut app, input("base", None));
        press(&mut app, "c");
        assert_eq!(pick_keys(&app), ["white_bg.out"]);
        key(&mut app, KeyCode::Esc);
        // A shape connects to a layer list.
        select(&mut app, input("layers", Some(0)));
        press(&mut app, "cblack");
        key(&mut app, KeyCode::Enter);
        assert!(read(&app, "comp").contains("layers = [\"placed\", \"black_sq\"]"));
        assert!(app.project.is_valid());
    }

    #[test]
    fn add_node() {
        let (_tmp, mut app) = temp_example();
        press(&mut app, "arectangle");
        key(&mut app, KeyCode::Enter);
        assert!(matches!(&app.mode, Mode::Prompt { input, .. } if input == "rectangle_1"));
        clear(&mut app);
        press(&mut app, "bad id");
        key(&mut app, KeyCode::Enter);
        assert!(prompt_error(&app).unwrap().contains("not a valid node id"));
        clear(&mut app);
        press(&mut app, "red_box");
        key(&mut app, KeyCode::Enter);
        assert_eq!(focus(&app), "red_box");
        assert!(read(&app, "red_box").starts_with("type = \"rectangle\""));
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
        let kinds = pick_keys(&app);
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
        let keys = pick_keys(&app);
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
        select(&mut app, param("fill_color"));
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
        // Optional params start from their default; transform.y can go negative.
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
        assert_eq!(int_param(&app, "black_sq", "width"), Some(0));
        press(&mut app, "-");
        assert_eq!(
            app.message.as_deref(),
            Some("black_sq.width is at its minimum, 0")
        );

        // Colors are adjusted in the color editor instead.
        select(&mut app, param("fill_color"));
        press(&mut app, "+");
        assert_eq!(
            app.message.as_deref(),
            Some("press ⏎ to adjust `fill_color` in the color editor")
        );
    }

    fn test_fonts(app: &mut App) {
        let family = |name: &str, monospace, italic| conduit_nodes::text::FontFamily {
            name: name.into(),
            monospace,
            italic,
        };
        app.fonts = Some(Arc::new(vec![
            family("Alpha Sans", false, true),
            family("Beta Mono", true, false),
            family("Gamma Serif", false, true),
        ]));
    }

    /// Opens the font picker on `node.font`, with a fixed font list (for view tests).
    pub(crate) fn open_font_picker_on(app: &mut App, node: &str) {
        test_fonts(app);
        goto(app, node);
        select(app, param("font"));
        key(app, KeyCode::Enter);
    }

    fn font_line(app: &App) -> Option<String> {
        read(app, "title")
            .lines()
            .find(|l| l.starts_with("font ="))
            .map(str::to_string)
    }

    #[test]
    fn font_picker_previews_in_place_and_reverts() {
        let (_tmp, mut app) = temp_project("text");
        test_fonts(&mut app);
        let original = read(&app, "title");
        goto(&mut app, "title");
        select(&mut app, param("font"));
        key(&mut app, KeyCode::Enter);
        // Generic names first, then families; the current font (the default) is selected.
        let labels: Vec<String> = app.pick_matches().into_iter().map(|(_, l)| l).collect();
        assert_eq!(
            labels,
            [
                "sans-serif  generic · current",
                "serif  generic",
                "monospace  generic",
                "Alpha Sans",
                "Beta Mono  mono",
                "Gamma Serif",
            ]
        );
        assert!(matches!(app.mode, Mode::Pick { selected: 0, .. }));

        // Moving writes the highlighted font straight away.
        key(&mut app, KeyCode::Down);
        assert_eq!(font_line(&app).as_deref(), Some("font = \"serif\""));
        key(&mut app, KeyCode::Down);
        assert_eq!(font_line(&app).as_deref(), Some("font = \"monospace\""));
        // esc puts the file back as it was.
        key(&mut app, KeyCode::Esc);
        assert_eq!(read(&app, "title"), original);
        assert_eq!(app.mode, Mode::Normal);

        // Filter, keep with ⏎; the whole session undoes in one step.
        key(&mut app, KeyCode::Enter);
        press(&mut app, "beta");
        assert_eq!(font_line(&app).as_deref(), Some("font = \"Beta Mono\""));
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(
            app.message.as_deref(),
            Some("set title.font = \"Beta Mono\"")
        );
        press(&mut app, "u");
        assert_eq!(read(&app, "title"), original);
        press(&mut app, "u");
        assert_eq!(app.message.as_deref(), Some("nothing to undo"));

        // Recent fonts come first next time.
        key(&mut app, KeyCode::Enter);
        let first = app.pick_matches().into_iter().next().unwrap();
        assert_eq!(first.1, "Beta Mono  recent · mono");
        key(&mut app, KeyCode::Esc);
    }

    /// `examples/image` with its images, plus `assets/other.png`.
    fn temp_image_project() -> (tempfile::TempDir, App) {
        let (tmp, app) = temp_project("image");
        let images =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/image/assets/images");
        std::fs::create_dir_all(tmp.path().join("assets/images")).unwrap();
        for name in ["badge.png", "sunset.jpg"] {
            std::fs::copy(
                images.join(name),
                tmp.path().join("assets/images").join(name),
            )
            .unwrap();
        }
        conduit_core::RgbaImage::new(3, 2)
            .save(tmp.path().join("assets/other.png"))
            .unwrap();
        (tmp, app)
    }

    fn path_line(app: &App) -> Option<String> {
        read(app, "photo")
            .lines()
            .find(|l| l.starts_with("path ="))
            .map(str::to_string)
    }

    #[test]
    fn file_picker_previews_in_place_and_reverts() {
        let (_tmp, mut app) = temp_image_project();
        let original = read(&app, "photo");
        goto(&mut app, "photo");
        select(&mut app, param("path"));
        key(&mut app, KeyCode::Enter);
        // Every image in the project by path, with its size; the current file is selected.
        let matches = app.pick_matches();
        let keys: Vec<&str> = matches.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            keys,
            [
                "assets/images/badge.png",
                "assets/images/sunset.jpg",
                "assets/other.png"
            ]
        );
        assert!(
            matches[1].1.contains("480×320 · 8 KB · current"),
            "{matches:?}"
        );
        assert!(
            matches[2].1.starts_with("assets/other.png  3×2 · "),
            "{matches:?}"
        );
        assert!(matches!(app.mode, Mode::Pick { selected: 1, .. }));

        // Moving writes the highlighted path straight away; esc puts it back.
        key(&mut app, KeyCode::Down);
        assert_eq!(
            path_line(&app).as_deref(),
            Some("path = \"assets/other.png\"")
        );
        key(&mut app, KeyCode::Esc);
        assert_eq!(read(&app, "photo"), original);

        // Filter, keep with ⏎; the session undoes in one step.
        key(&mut app, KeyCode::Enter);
        press(&mut app, "badge");
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(
            app.message.as_deref(),
            Some("set photo.path = \"assets/images/badge.png\"")
        );
        press(&mut app, "u");
        assert_eq!(read(&app, "photo"), original);
    }

    #[test]
    fn feeding_a_font_param_adds_a_font_node_and_picks_its_file() {
        let (tmp, mut app) = temp_project("text");
        let fonts = tmp.path().join("assets/fonts");
        std::fs::create_dir_all(&fonts).unwrap();
        let droid = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/conduit-nodes/tests/fixtures/fonts/DroidSans.ttf");
        std::fs::copy(droid, fonts.join("DroidSans.ttf")).unwrap();

        // `i` on text.font offers node types with a font output.
        goto(&mut app, "title");
        select(&mut app, param("font"));
        press(&mut app, "i");
        let keys = pick_keys(&app);
        assert_eq!(keys, ["font"]);
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Enter); // accept the suggested id

        // The new node is focused with the file picker open on its path.
        assert_eq!(app.focus.as_deref(), Some("title_font"));
        let matches = app.pick_matches();
        assert_eq!(matches.len(), 1, "{matches:?}");
        assert_eq!(matches[0].0, "assets/fonts/DroidSans.ttf");
        assert!(matches[0].1.contains("Droid Sans · 190 KB"), "{matches:?}");
        key(&mut app, KeyCode::Enter);
        assert!(read(&app, "title_font").contains("path = \"assets/fonts/DroidSans.ttf\""));
        assert!(read(&app, "title").contains("font = \"title_font\""));
        let title = app.project.get("title").unwrap();
        assert_eq!(title.inputs["font"][0].node, "title_font");
    }

    #[test]
    fn file_picker_without_files_opens_a_prompt() {
        let (_tmp, mut app) = temp_project("image");
        goto(&mut app, "photo");
        select(&mut app, param("path"));
        key(&mut app, KeyCode::Enter);
        assert!(
            matches!(&app.mode, Mode::Prompt { input, .. } if input == "assets/images/sunset.jpg"),
            "{:?}",
            app.mode
        );
        assert_eq!(
            app.message.as_deref(),
            Some("no .png/.jpg/.jpeg/.webp/.gif/.bmp files in the project folder; type a path")
        );
    }

    #[test]
    fn font_picker_flags_families_without_italic() {
        let (_tmp, mut app) = temp_project("text");
        test_fonts(&mut app);
        goto(&mut app, "note");
        select(&mut app, param("font"));
        key(&mut app, KeyCode::Enter);
        let labels: Vec<String> = app.pick_matches().into_iter().map(|(_, l)| l).collect();
        assert!(
            labels.contains(&"Beta Mono  mono · no italic".to_string()),
            "{labels:?}"
        );
        assert!(
            labels.contains(&"serif  generic · current".to_string()),
            "{labels:?}"
        );
        key(&mut app, KeyCode::Esc);
    }

    #[test]
    fn reorder_composite_layers() {
        let (_tmp, mut app) = temp_project("transform");
        goto(&mut app, "comp");
        select(&mut app, input("layers", Some(0)));
        // `J` raises `turned` one place; the cursor follows it.
        press(&mut app, "J");
        assert!(
            read(&app, "comp").contains(r#"layers = ["slanted", "turned", "baked"]"#),
            "{}",
            read(&app, "comp")
        );
        assert_eq!(
            app.message.as_deref(),
            Some("moved comp.layers ← turned to [1]")
        );
        press(&mut app, "J");
        assert!(read(&app, "comp").contains(r#"["slanted", "baked", "turned"]"#));
        press(&mut app, "J");
        assert_eq!(
            app.message.as_deref(),
            Some("comp.layers[2] is already last")
        );
        press(&mut app, "K");
        assert!(read(&app, "comp").contains(r#"["slanted", "turned", "baked"]"#));

        press(&mut app, "uuu");
        assert!(read(&app, "comp").contains(r#"["turned", "slanted", "baked"]"#));

        select(&mut app, input("base", None));
        press(&mut app, "J");
        assert_eq!(
            app.message.as_deref(),
            Some("move the cursor to a connection in a list to reorder it")
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
            matches!(&app.mode, Mode::Confirm { question, .. } if question.contains("stripe.fill_color"))
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
        assert_eq!(conduit_core::to_hex(color_edit(&app).color()), "#e76f51");
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
        select(&mut app, param("fill_color"));
        key(&mut app, KeyCode::Enter);
        press(&mut app, "#123456");
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Enter);
        assert_eq!(link(&app, "fill_color"), None);
        assert!(read(&app, "square").contains("fill_color = \"#123456\""));
    }

    #[test]
    fn open_the_picker() {
        let (_tmp, mut app) = temp_project("palette");
        goto(&mut app, "brand");
        select(&mut app, entry("accent"));
        let Some(Action::OpenPicker(args)) = press(&mut app, "C") else {
            panic!("expected the picker to open: {:?}", app.message);
        };
        let dir = app.dir.to_string_lossy().into_owned();
        assert_eq!(args, [dir.as_str(), "brand", "accent", "--table", "colors"]);

        // What the picker writes undoes in one step.
        let original = read(&app, "brand");
        std::fs::write(app.dir.join("brand.toml"), "type = \"palette\"").unwrap();
        press(&mut app, "u");
        assert_eq!(read(&app, "brand"), original);

        goto(&mut app, "square");
        select(&mut app, param("fill_color"));
        let Some(Action::OpenPicker(args)) = press(&mut app, "C") else {
            panic!("expected the picker to open");
        };
        assert_eq!(&args[1..], ["square", "fill_color"]);
        select(&mut app, param("width"));
        assert_eq!(press(&mut app, "C"), None);
        assert!(
            app.message
                .as_deref()
                .unwrap()
                .contains("move the cursor to a color")
        );
    }

    #[test]
    fn feed_a_color_param_from_a_color_node() {
        let (_tmp, mut app) = temp_example();
        goto(&mut app, "black_sq");
        select(&mut app, param("fill_color"));
        press(&mut app, "i");
        let kinds = pick_keys(&app);
        assert_eq!(kinds, ["color"], "palettes only have named outputs");
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Enter); // `black_sq_fill_color`
        assert!(read(&app, "black_sq_fill_color").contains("color = \"black\""));
        assert_eq!(
            link(&app, "fill_color"),
            Some(reference("black_sq_fill_color"))
        );
        assert!(app.project.is_valid(), "{:?}", app.project.errors);
    }

    #[test]
    fn insert_node_into_connection() {
        let (_tmp, mut app) = temp_example();
        goto(&mut app, "comp");
        select(&mut app, input("base", None));
        press(&mut app, "i");
        let kinds = pick_keys(&app);
        assert_eq!(kinds, ["canvas", "composite"], "image → image nodes fit");
        press(&mut app, "composite");
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Enter); // accept `composite_1`
        assert_eq!(focus(&app), "composite_1");
        assert!(read(&app, "comp").contains("base = \"composite_1\""));
        assert!(read(&app, "composite_1").contains("base = \"white_bg\""));
        assert!(app.project.is_valid(), "{:?}", app.project.errors);
    }

    /// Moves into `pane` and puts the cursor on the first row for `node`.
    fn select_tree(app: &mut App, pane: Pane, node: &str) {
        app.pane = pane;
        let tree = if pane == Pane::Up { &app.up } else { &app.down };
        let i = app
            .visible(pane)
            .iter()
            .position(|&i| tree[i].node == node)
            .unwrap_or_else(|| panic!("{node} in the {pane:?} tree"));
        app.set_cursor(pane, i);
    }

    /// Picks the node type `kind` in the open picker and accepts the suggested id.
    fn pick_type(app: &mut App, kind: &str) {
        press(app, kind);
        assert_eq!(app.pick_matches()[0].0, kind);
        key(app, KeyCode::Enter);
        key(app, KeyCode::Enter);
    }

    #[test]
    fn insert_from_the_trees() {
        let (_tmp, mut app) = temp_project("effects");
        // Inputs tree, depth 2: `bar` feeds `bar_turned.layer`.
        goto(&mut app, "bar_inverted");
        select_tree(&mut app, Pane::Up, "bar");
        press(&mut app, "i");
        pick_type(&mut app, "hsl");
        assert_eq!(focus(&app), "hsl_1");
        assert!(read(&app, "bar_turned").contains("layer = \"hsl_1\""));
        assert!(read(&app, "hsl_1").contains("layer = \"bar\""));

        // Used-by tree: `swatch_at` feeds `comp.layers[1]`.
        goto(&mut app, "swatch_shifted");
        select_tree(&mut app, Pane::Down, "comp");
        press(&mut app, "i");
        pick_type(&mut app, "invert");
        assert!(read(&app, "invert_1").contains("layer = \"swatch_at\""));
        assert!(
            read(&app, "comp").contains("[\"logo_at\", \"invert_1\", \"bar_inverted\"]"),
            "{}",
            read(&app, "comp")
        );
        assert!(app.project.is_valid(), "{:?}", app.project.errors);
    }

    #[test]
    fn dissolve_reconnects() {
        let (_tmp, mut app) = temp_project("effects");
        goto(&mut app, "bar_turned");
        press(&mut app, "X");
        assert!(
            matches!(&app.mode, Mode::Confirm { question, .. }
                if question.contains("Connects bar → bar_inverted.layer")),
            "{:?}",
            app.mode
        );
        press(&mut app, "y");
        assert!(read(&app, "bar_turned").is_empty());
        assert!(read(&app, "bar_inverted").contains("layer = \"bar\""));
        assert!(app.project.is_valid(), "{:?}", app.project.errors);
        press(&mut app, "u");
        assert!(!read(&app, "bar_turned").is_empty());

        // From a tree, the highlighted node goes and focus stays.
        goto(&mut app, "comp");
        select_tree(&mut app, Pane::Up, "logo_tinted");
        press(&mut app, "Xy");
        assert_eq!(focus(&app), "comp");
        assert!(read(&app, "logo_tinted").is_empty());
        assert!(read(&app, "logo_at").contains("layer = \"logo\""));
    }

    #[test]
    fn delete_in_a_tree_deletes_the_highlighted_node() {
        let (_tmp, mut app) = temp_project("effects");
        goto(&mut app, "comp");
        select_tree(&mut app, Pane::Up, "swatch_at");
        press(&mut app, "Dy");
        assert_eq!(focus(&app), "comp");
        assert!(read(&app, "swatch_at").is_empty());
        assert!(!read(&app, "comp").is_empty());
    }

    #[test]
    fn move_a_node_to_another_connection() {
        let (_tmp, mut app) = temp_project("effects");
        goto(&mut app, "bar_turned");
        press(&mut app, "m");
        assert_eq!(app.carrying.as_deref(), Some("bar_turned"));

        // Not a connection: still carrying.
        select(&mut app, param("x"));
        key(&mut app, KeyCode::Enter);
        assert_eq!(app.carrying.as_deref(), Some("bar_turned"));
        assert!(read(&app, "bar_inverted").contains("layer = \"bar_turned\""));

        // `bar_inverted → comp.layers[2]`, two levels down the used-by tree.
        select_tree(&mut app, Pane::Down, "comp");
        press(&mut app, "m");
        assert_eq!(app.carrying, None, "{:?}", app.message);
        assert!(read(&app, "bar_inverted").contains("layer = \"bar\""));
        assert!(read(&app, "bar_turned").contains("layer = \"bar_inverted\""));
        assert!(
            read(&app, "comp").contains("\"bar_turned\"]"),
            "{}",
            read(&app, "comp")
        );
        assert!(app.project.is_valid(), "{:?}", app.project.errors);

        // One undo puts it all back.
        press(&mut app, "u");
        assert!(read(&app, "bar_inverted").contains("layer = \"bar_turned\""));

        // esc cancels.
        press(&mut app, "m");
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.carrying, None);
    }

    #[test]
    fn typing_replaces_the_highlighted_value() {
        let (_tmp, mut app) = temp_example();
        goto(&mut app, "black_sq");
        select(&mut app, param("width"));
        key(&mut app, KeyCode::Enter);
        assert!(matches!(&app.mode, Mode::Prompt { input, selected: true, .. } if input == "50"));
        press(&mut app, "80");
        assert!(matches!(&app.mode, Mode::Prompt { input, selected: false, .. } if input == "80"));
        key(&mut app, KeyCode::Enter);
        assert!(read(&app, "black_sq").contains("width = 80"));

        // → keeps the value to edit it; backspace on a highlighted value clears it.
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Right);
        press(&mut app, "0");
        assert!(matches!(&app.mode, Mode::Prompt { input, .. } if input == "800"));
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::Enter);
        key(&mut app, KeyCode::Backspace);
        assert!(matches!(&app.mode, Mode::Prompt { input, .. } if input.is_empty()));
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
