//! The flow view's edits (spec §4.4, §6.2–§6.7): what each verb does to the focus, and
//! where the flow and focus go once the edit has landed. Also flow history (`[` / `]`).
//!
//! An edit often goes through a picker or prompt first, and may be cancelled or fail.
//! So a verb records a [`Pending`] edit — the flow and focus before, and what should
//! happen after — and [`App::finish_flow_edit`] applies it once the popups have closed
//! and the edit was recorded for undo. Otherwise it's dropped.

use conduit_core::Consumer;
use conduit_core::edit::{self as ops, Connection};
use conduit_core::{Project, Reference};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::{Action, App, Mode, Pane, Pick, Prompt};
use crate::edit::flow::{
    Flow, Focus, InState, Link, input_rows, is_flow_input, is_flow_output, output_rows,
};

/// Where the flow and focus go after an edit.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::edit) enum After {
    /// Redraw the flow through input `node.port[index]` and focus that end (§4.4).
    ThroughInput {
        node: String,
        port: String,
        index: usize,
    },
    /// Redraw the downstream side through `node.output`'s new user (one not in
    /// `before`) and focus that output row (§6.3 `(c)onnect to`, `i` on an output end).
    NewUser {
        node: String,
        output: String,
        before: Vec<Consumer>,
    },
    /// The flow keeps its path without `id`; focus the conduit that closed the gap
    /// (§6.7). If nothing bridges it, as for a delete.
    Dissolved { id: String },
    /// `id`'s downstream neighbour (upstream for a sink) becomes anchor and focus.
    Deleted { id: String },
    /// A connection was removed: `target.port[index]` (index `None` for a single input).
    /// Focus the row at `pos` (inputs, then outputs) of `node`, or the one above, or the
    /// node. `flow_into`: it was the flow's conduit, so the upstream side is rebuilt.
    Disconnected {
        target: String,
        port: String,
        index: Option<usize>,
        node: String,
        pos: usize,
        flow_into: bool,
    },
    /// Same flow under the new name (`self.focus`).
    Renamed { old: String },
    /// List entries `i` and `j` of `node.port` swapped; focus follows the entry.
    Swapped {
        node: String,
        port: String,
        i: usize,
        j: usize,
    },
    /// `id` was detached; the flow is rebuilt around `landing`, the new node that took
    /// over the output the flow ran through (else the first new node).
    Detached { id: String, landing: String },
    /// The moved node landed; the flow is redrawn through it (§6.5).
    Dropped { id: String },
    /// The flow is now `flow` with the focus on `focus` (a slide, §6.5).
    Replace { flow: Flow, focus: Focus },
}

#[derive(Debug, Clone)]
pub(in crate::edit) struct Pending {
    undo_len: usize,
    flow: Flow,
    focus: Focus,
    after: After,
}

/// The row's position among `node`'s rows: inputs, then outputs.
fn row_position(project: &Project, focus: &Focus) -> Option<usize> {
    let node = project.get(focus.node())?;
    let ins = input_rows(node);
    match focus {
        Focus::In { row, .. } => ins
            .iter()
            .position(|r| r.port == row.port && r.index == row.index),
        Focus::Out { row, .. } => output_rows(project, node)
            .iter()
            .position(|r| r.port == row.port && r.user == row.user)
            .map(|p| p + ins.len()),
        _ => None,
    }
}

/// The focus on row `pos` of `node` (inputs, then outputs), else the row above, else the
/// node itself (§6.7).
fn row_at(project: &Project, node: &str, pos: usize) -> Focus {
    let Some(n) = project.get(node) else {
        return Focus::Node(node.to_string());
    };
    let ins = input_rows(n);
    let outs = output_rows(project, n);
    let total = ins.len() + outs.len();
    if total == 0 {
        return Focus::Node(node.to_string());
    }
    let pos = pos.min(total - 1);
    if pos < ins.len() {
        Focus::In {
            node: node.to_string(),
            row: ins[pos].clone(),
        }
    } else {
        Focus::Out {
            node: node.to_string(),
            row: outs[pos - ins.len()].clone(),
        }
    }
}

/// The input row `node.port[index]` while connected.
fn input_end(project: &Project, node: &str, port: &str, index: usize) -> Option<Focus> {
    let n = project.get(node)?;
    let row = input_rows(n).into_iter().find(|r| {
        r.port == port && r.index.unwrap_or(0) == index && matches!(r.state, InState::Connected(_))
    })?;
    Some(Focus::In {
        node: node.to_string(),
        row,
    })
}

/// Whether every node and conduit of `flow` is still in `project`.
fn still_valid(project: &Project, flow: &Flow) -> bool {
    flow.nodes.iter().all(|n| project.get(n).is_some())
        && flow.links.iter().all(|l| {
            project
                .get(&l.target)
                .and_then(|n| n.inputs.get(&l.port))
                .and_then(|refs| refs.get(l.index))
                .is_some_and(|r| r.node == l.source && r.port == l.output)
        })
}

impl App {
    /// Records what should happen to the flow once the next edit lands.
    fn expect(&mut self, after: After) {
        if let Some(focus) = self.flow_focus.clone() {
            self.pending = Some(Pending {
                undo_len: self.undo.len(),
                flow: self.flow.clone(),
                focus,
                after,
            });
        }
    }

    /// Records the flow and focus before a flow change, for `[` (§6.1).
    pub(in crate::edit) fn remember_flow(&mut self, flow: Flow, focus: Focus) {
        if flow.nodes.is_empty() {
            return;
        }
        self.flow_back.push((flow, focus));
        self.flow_forward.clear();
    }

    /// Moves the focus onto `flow`; a flow change re-centres and is a history step.
    fn land(&mut self, flow: Flow, focus: Focus, change: Option<(Flow, Focus)>) {
        if let Some((old_flow, old_focus)) = change {
            self.remember_flow(old_flow, old_focus);
            self.recentre = true;
        }
        self.flow = flow;
        self.focus = Some(focus.node().to_string());
        self.flow_focus = Some(focus);
        self.refresh();
    }

    /// Applies the pending edit's flow and focus once its popups have closed; drops it
    /// if the edit was cancelled or failed.
    pub(in crate::edit) fn finish_flow_edit(&mut self) {
        if self.mode != Mode::Normal {
            return;
        }
        let Some(p) = self.pending.take() else {
            return;
        };
        if self.undo.len() <= p.undo_len {
            return;
        }
        let before = Some((p.flow.clone(), p.focus.clone()));
        let project = &self.project;
        let mut flow = p.flow.clone();
        match p.after {
            After::ThroughInput { node, port, index } => {
                if flow.position(&node).is_none() {
                    flow = Flow::build(project, &node);
                }
                // Redrawing upstream through the end also replaces any conduits the edit
                // changed on that side.
                let i = flow.position(&node).unwrap_or(0);
                flow.through_input(project, i, &port, index);
                let focus = input_end(project, &node, &port, index).unwrap_or(Focus::Node(node));
                self.land(flow, focus, before);
            }
            After::NewUser {
                node,
                output,
                before: users,
            } => {
                let Some(user) = project
                    .consumers(&node)
                    .iter()
                    .find(|c| c.output == output && !users.contains(c))
                    .cloned()
                else {
                    return;
                };
                if !still_valid(project, &flow) || flow.position(&node).is_none() {
                    flow = Flow::build(project, &node);
                }
                let i = flow.position(&node).unwrap_or(0);
                flow.through_output(project, i, &user);
                let focus = Focus::Out {
                    node: node.clone(),
                    row: crate::edit::flow::OutRow {
                        port: output,
                        user: Some(user),
                    },
                };
                self.land(flow, focus, before);
            }
            After::Dissolved { id } => {
                if let Some((flow, focus)) = bridged(project, &flow, &id) {
                    self.land(flow, focus, None);
                } else {
                    self.after_delete(&flow, &id, before);
                }
            }
            After::Deleted { id } => self.after_delete(&flow, &id, before),
            After::Disconnected {
                target,
                port,
                index,
                node,
                pos,
                flow_into,
            } => {
                if flow_into {
                    let i = flow.position(&node).unwrap_or(0);
                    flow.rebuild_upstream(project, i);
                    let focus = row_at(project, &node, pos);
                    self.land(flow, focus, before);
                } else {
                    // A removed list entry renumbers the ones after it.
                    if let Some(k) = index {
                        for l in &mut flow.links {
                            if l.target == target && l.port == port && l.index > k {
                                l.index -= 1;
                            }
                        }
                    }
                    if !still_valid(project, &flow) {
                        flow = Flow::build(project, &node);
                    }
                    let focus = row_at(project, &node, pos);
                    self.land(flow, focus, None);
                }
            }
            After::Renamed { old } => {
                let Some(new) = self.focus.clone() else {
                    return;
                };
                let rename = |s: &mut String| {
                    if *s == old {
                        s.clone_from(&new);
                    }
                };
                flow.nodes.iter_mut().for_each(rename);
                for l in &mut flow.links {
                    rename(&mut l.source);
                    rename(&mut l.target);
                }
                let focus = match p.focus {
                    Focus::Conduit(mut l) => {
                        rename(&mut l.source);
                        rename(&mut l.target);
                        Focus::Conduit(l)
                    }
                    _ => Focus::Node(new.clone()),
                };
                let focus = flow.locate(project, &focus).unwrap_or(Focus::Node(new));
                self.land(flow, focus, None);
            }
            After::Swapped { node, port, i, j } => {
                for l in &mut flow.links {
                    if l.target == node && l.port == port {
                        if l.index == i {
                            l.index = j;
                        } else if l.index == j {
                            l.index = i;
                        }
                    }
                }
                let focus = input_end(project, &node, &port, j).unwrap_or(Focus::Node(node));
                self.land(flow, focus, None);
            }
            After::Replace { flow, focus } => {
                if still_valid(project, &flow) {
                    self.land(flow, focus, None);
                }
            }
            After::Detached { id, landing } => {
                if self.project.get(&id).is_some() || self.project.get(&landing).is_none() {
                    return; // cancelled or failed
                }
                let flow = Flow::build(project, &landing);
                self.land(flow, Focus::Node(landing), before);
            }
            After::Dropped { id } => {
                let flow = Flow::build(project, &id);
                self.land(flow, Focus::Node(id), before);
            }
        }
    }

    /// After `id` was deleted (§6.7): its downstream neighbour, else its upstream one,
    /// becomes the anchor and focus. Deleting a node outside the flow leaves it as is.
    fn after_delete(&mut self, flow: &Flow, id: &str, before: Option<(Flow, Focus)>) {
        let Some(i) = flow.position(id) else {
            return;
        };
        let anchor = flow
            .nodes
            .get(i + 1)
            .or(i.checked_sub(1).and_then(|k| flow.nodes.get(k)))
            .filter(|n| self.project.get(n).is_some())
            .cloned()
            .or_else(|| self.default_focus());
        match anchor {
            Some(anchor) => {
                let flow = Flow::build(&self.project, &anchor);
                self.land(flow, Focus::Node(anchor), before);
            }
            None => {
                if let Some((flow, focus)) = before {
                    self.remember_flow(flow, focus);
                }
                self.flow = Flow::default();
                self.flow_focus = None;
                self.focus = None;
                self.refresh();
            }
        }
    }

    // --- keys -------------------------------------------------------------------------

    /// The verbs (§6.2) on the focus. `None` if the key isn't one.
    pub(super) fn flow_verb(&mut self, key: KeyEvent, focus: Focus) -> Option<Option<Action>> {
        if let Some(id) = self.carrying.clone() {
            return Some(self.moving_key(key, &focus, id));
        }
        let primary = key.code == KeyCode::Enter;
        let KeyCode::Char(c) = key.code else {
            if primary {
                return Some(self.primary(&focus));
            }
            return None;
        };
        match c {
            'e' => self.edit(&focus),
            'c' => self.connect(&focus),
            'i' => self.insert(&focus),
            'd' => self.remove(&focus),
            'D' => {
                self.expect(After::Deleted {
                    id: focus.node().to_string(),
                });
                self.pane = Pane::Node;
                self.start_delete();
            }
            'r' => {
                let id = focus.node().to_string();
                self.expect(After::Renamed { old: id.clone() });
                self.open_prompt(Prompt::Rename, id);
            }
            'X' => self.detach(&focus),
            'K' => self.reorder(&focus, -1),
            'J' => self.reorder(&focus, 1),
            'm' => self.pick_up(&focus),
            'H' | 'L' => {
                if let Focus::Node(id) = &focus {
                    self.slide(id, c == 'H');
                }
            }
            _ => return None,
        }
        Some(None)
    }

    /// `X` (§6.2): detach the focus's node (asks first).
    fn detach(&mut self, focus: &Focus) {
        let id = focus.node().to_string();
        let Some(plan) = self.start_detach(&id) else {
            return;
        };
        // The output the flow leaves `id` through, if any.
        let through = self
            .flow
            .links
            .iter()
            .find(|l| l.source == id)
            .map(|l| l.output.clone());
        let landing = through
            .and_then(|out| plan.outputs.iter().find(|(o, _)| *o == out))
            .or(plan.outputs.first())
            .map(|(_, name)| format!("{id}_{name}"));
        if let Some(landing) = landing {
            self.expect(After::Detached { id, landing });
        }
    }

    /// `⏎`: the focus's primary action (§6.3).
    fn primary(&mut self, focus: &Focus) -> Option<Action> {
        match focus {
            Focus::Node(_) => self.edit(focus),
            Focus::In { node, row } => {
                let i = self.flow.position(node).unwrap_or(0);
                match row.state {
                    InState::Connected(_) if !is_flow_input(&self.flow, i, row) => {
                        return self.bring_in(focus);
                    }
                    _ => self.connect(focus),
                }
            }
            Focus::Out { node, row } => {
                let i = self.flow.position(node).unwrap_or(0);
                if row.user.is_some() && !is_flow_output(&self.flow, i, row) {
                    return self.bring_in(focus);
                }
                self.connect(focus);
            }
            Focus::Conduit(_) => self.connect(focus),
        }
        None
    }

    /// `f` on a `▣` end (§4.4): a flow change.
    pub(super) fn bring_in(&mut self, focus: &Focus) -> Option<Action> {
        let mut flow = self.flow.clone();
        if flow.bring_in(&self.project, focus).is_some() {
            let located = flow.locate(&self.project, focus).unwrap_or(focus.clone());
            let before = Some((self.flow.clone(), focus.clone()));
            self.land(flow, located, before);
        }
        None
    }

    /// `e`: open the edit view on a node (§7).
    fn edit(&mut self, focus: &Focus) {
        if let Focus::Node(_) = focus {
            self.open_edit_view();
        }
    }

    /// `c` (§6.2): re-pick the source of an input end or conduit; append to a list's
    /// slot; on an output end, pick a new user.
    fn connect(&mut self, focus: &Focus) {
        match focus {
            Focus::Node(_) => {}
            Focus::Conduit(l) => self.connect_input(&l.target, &l.port, Some(l.index)),
            Focus::In { node, row } => match row.state {
                InState::Connected(_) => {
                    self.connect_input(node, &row.port, Some(row.index.unwrap_or(0)))
                }
                _ => self.connect_input(node, &row.port, None),
            },
            Focus::Out { node, row } => {
                self.expect(After::NewUser {
                    node: node.clone(),
                    output: row.port.clone(),
                    before: self.project.consumers(node).to_vec(),
                });
                self.open_pick(Pick::ConnectTo {
                    output: row.port.clone(),
                });
            }
        }
    }

    /// Picks a source for `node.port`: entry `index` if connected, else a new one.
    fn connect_input(&mut self, node: &str, port: &str, index: Option<usize>) {
        let multi = self.is_multi(node, port);
        let entries = self.entries(node, port);
        self.expect(After::ThroughInput {
            node: node.to_string(),
            port: port.to_string(),
            index: index.unwrap_or(if multi { entries } else { 0 }),
        });
        self.open_pick(Pick::Connect {
            port: port.to_string(),
            multi,
            index: index.filter(|_| multi),
        });
    }

    /// `i` (§6.2): a new node into a conduit, or feeding an unconnected input or slot.
    fn insert(&mut self, focus: &Focus) {
        match focus {
            Focus::Node(_) => {}
            Focus::Conduit(l) => self.insert_into(&l.target, &l.port, l.index),
            Focus::In { node, row } => match row.state {
                InState::Connected(_) => self.insert_into(node, &row.port, row.index.unwrap_or(0)),
                _ => {
                    let index = if self.is_multi(node, &row.port) {
                        self.entries(node, &row.port)
                    } else {
                        0
                    };
                    self.expect(After::ThroughInput {
                        node: node.clone(),
                        port: row.port.clone(),
                        index,
                    });
                    self.open_pick(Pick::FeedInput {
                        port: row.port.clone(),
                    });
                }
            },
            Focus::Out { node, row } => {
                let Some(user) = &row.user else { return };
                self.expect(After::NewUser {
                    node: node.clone(),
                    output: row.port.clone(),
                    before: self.project.consumers(node).to_vec(),
                });
                self.open_pick(Pick::InsertType {
                    node: user.node.clone(),
                    port: user.port.clone(),
                    index: user.index,
                });
            }
        }
    }

    fn insert_into(&mut self, node: &str, port: &str, index: usize) {
        self.expect(After::ThroughInput {
            node: node.to_string(),
            port: port.to_string(),
            index,
        });
        self.open_pick(Pick::InsertType {
            node: node.to_string(),
            port: port.to_string(),
            index,
        });
    }

    /// `d` (§6.2, §6.7): dissolve a node (asks first); disconnect a conduit or end.
    fn remove(&mut self, focus: &Focus) {
        let flow = self.flow.clone();
        let (target, port, index, flow_into, node, pos) = match focus {
            Focus::Node(id) => {
                self.expect(After::Dissolved { id: id.clone() });
                self.pane = Pane::Node;
                self.start_dissolve();
                return;
            }
            Focus::Conduit(l) => {
                let Some(end) = input_end(&self.project, &l.target, &l.port, l.index) else {
                    return;
                };
                let pos = row_position(&self.project, &end).unwrap_or(0);
                (
                    l.target.clone(),
                    l.port.clone(),
                    l.index,
                    true,
                    l.target.clone(),
                    pos,
                )
            }
            Focus::In { node, row } => {
                if !matches!(row.state, InState::Connected(_)) {
                    return;
                }
                let i = flow.position(node).unwrap_or(0);
                let pos = row_position(&self.project, focus).unwrap_or(0);
                let index = row.index.unwrap_or(0);
                let into = is_flow_input(&flow, i, row);
                (
                    node.clone(),
                    row.port.clone(),
                    index,
                    into,
                    node.clone(),
                    pos,
                )
            }
            Focus::Out { node, row } => {
                let Some(user) = &row.user else { return };
                let i = flow.position(node).unwrap_or(0);
                if is_flow_output(&flow, i, row) {
                    // The flow's conduit: focus moves to the downstream node (§6.7).
                    let Some(end) = input_end(&self.project, &user.node, &user.port, user.index)
                    else {
                        return;
                    };
                    let pos = row_position(&self.project, &end).unwrap_or(0);
                    let t = user.node.clone();
                    (t.clone(), user.port.clone(), user.index, true, t, pos)
                } else {
                    let pos = row_position(&self.project, focus).unwrap_or(0);
                    (
                        user.node.clone(),
                        user.port.clone(),
                        user.index,
                        false,
                        node.clone(),
                        pos,
                    )
                }
            }
        };
        let list = self.is_multi(&target, &port).then_some(index);
        let source = self
            .project
            .get(&target)
            .and_then(|n| n.inputs.get(&port)?.get(index).cloned());
        let entry = list.map_or(port.clone(), |i| format!("{port}[{i}]"));
        let label = format!(
            "disconnected {target}.{entry} ← {}",
            source.map(|s| s.short()).unwrap_or_default()
        );
        self.expect(After::Disconnected {
            target: target.clone(),
            port: port.clone(),
            index: list,
            node,
            pos,
            flow_into,
        });
        self.apply_or_report(label, |dir, _| ops::disconnect(dir, &target, &port, list));
    }

    /// `K` / `J` (§6.2): move a list entry one place earlier / later.
    fn reorder(&mut self, focus: &Focus, step: isize) {
        let Focus::In { node, row } = focus else {
            return;
        };
        let (Some(i), InState::Connected(source)) = (row.index, &row.state) else {
            return;
        };
        let entries = self.entries(node, &row.port);
        let Some(j) = i.checked_add_signed(step).filter(|j| *j < entries) else {
            return;
        };
        let (node, port) = (node.clone(), row.port.clone());
        let label = format!("moved {node}.{port} ← {} to [{j}]", source.short());
        self.expect(After::Swapped {
            node: node.clone(),
            port: port.clone(),
            i,
            j,
        });
        self.apply_or_report(label, |dir, _| {
            ops::swap_connections(dir, &node, &port, i, j)
        });
    }

    /// `H` / `L` (§6.5): swap the node with its flow neighbour upstream / downstream.
    fn slide(&mut self, id: &str, up: bool) {
        match self.slide_plan(id, up) {
            Ok((conns, flow, label)) => {
                self.expect(After::Replace {
                    flow,
                    focus: Focus::Node(id.to_string()),
                });
                self.apply_or_report(label, |dir, project| {
                    ops::swap_neighbours(dir, project, &conns[0], &conns[1], &conns[2])
                });
            }
            Err(e) => self.message = Some(e),
        }
    }

    /// What sliding `id` would do: the three connections to rewire, the flow after, and
    /// the status message; or why it can't (§6.5: not past either end of the flow, and
    /// only where the types fit).
    pub(super) fn slide_plan(
        &self,
        id: &str,
        up: bool,
    ) -> Result<([Connection; 3], Flow, String), String> {
        let flow = &self.flow;
        let i = flow.position(id).ok_or("not on the flow")?;
        let last = flow.nodes.len() - 1;
        // `x` is the upstream one of the two that swap.
        let k = if up { i.checked_sub(1) } else { Some(i) };
        let ok = k.is_some_and(|k| k >= 1 && k + 2 <= last);
        let other = if up { i.checked_sub(1) } else { Some(i + 1) }
            .and_then(|j| flow.nodes.get(j))
            .cloned();
        let place = if up { "before" } else { "after" };
        // (what's refused, what's done)
        let label = |other: &str| {
            (
                format!("slide {id} {place} {other}"),
                format!("slid {id} {place} {other}"),
            )
        };
        let Some(k) = k.filter(|_| ok) else {
            let end = if up { "start" } else { "end" };
            return Err(match other {
                Some(other) => format!(
                    "can't {}: {other} is at the {end} of the flow",
                    label(&other).0
                ),
                None => format!("can't slide {id}: it's at the {end} of the flow"),
            });
        };
        let other = other.expect("checked");
        let conn = |l: &Link| Connection {
            source: Reference {
                node: l.source.clone(),
                port: l.output.clone(),
            },
            node: l.target.clone(),
            port: l.port.clone(),
            index: l.index,
        };
        let (a, b, c) = (&flow.links[k - 1], &flow.links[k], &flow.links[k + 1]);
        let conns = [conn(a), conn(b), conn(c)];
        ops::check_swap(&self.project, &conns[0], &conns[1], &conns[2])
            .map_err(|e| format!("can't {}: {e}", label(&other).0))?;
        let mut after = flow.clone();
        after.nodes.swap(k, k + 1);
        let (x, y) = (&a.target, &b.target);
        after.links[k - 1] = Link {
            source: a.source.clone(),
            output: a.output.clone(),
            target: y.clone(),
            port: b.port.clone(),
            index: b.index,
        };
        after.links[k] = Link {
            source: y.clone(),
            output: c.output.clone(),
            target: x.clone(),
            port: a.port.clone(),
            index: a.index,
        };
        after.links[k + 1] = Link {
            source: x.clone(),
            output: b.output.clone(),
            target: c.target.clone(),
            port: c.port.clone(),
            index: c.index,
        };
        Ok((conns, after, label(&other).1))
    }

    /// `m` on a node (§6.5): pick it up, to drop into a conduit with `m` or `⏎`.
    fn pick_up(&mut self, focus: &Focus) {
        let Focus::Node(id) = focus else {
            return;
        };
        let node = self.project.get(id);
        let fed = node.is_some_and(|n| {
            input_rows(n)
                .iter()
                .any(|r| matches!(r.state, InState::Connected(_)))
        });
        let output = node.is_some_and(|n| !n.outputs().is_empty());
        if !fed || !output {
            let why = if !fed {
                "nothing feeds it"
            } else {
                "it has no output"
            };
            self.message = Some(format!("can't move {id}: {why}"));
            return;
        }
        if let Err(e) = ops::bypass_plan(&self.project, id) {
            self.message = Some(format!("can't move {id}: {e}"));
            return;
        }
        self.carrying = Some(id.clone());
    }

    /// Keys while a node is picked up: walk, `m`/`⏎` drop, `esc` cancel (§6.5).
    fn moving_key(&mut self, key: KeyEvent, focus: &Focus, id: String) -> Option<Action> {
        match key.code {
            KeyCode::Esc => {
                self.carrying = None;
                self.message = Some(format!("stopped moving {id}"));
            }
            KeyCode::Char('m') | KeyCode::Enter => {
                let Some(target) = self.connection_at(focus) else {
                    self.message = Some(format!(
                        "moving {id}: go to a conduit or end to drop it there, or esc"
                    ));
                    return None;
                };
                let label = format!(
                    "moved {id} to {} → {}.{}",
                    target.source.short(),
                    target.node,
                    target.port
                );
                self.expect(After::Dropped { id: id.clone() });
                match self.apply(label, |dir, project| {
                    ops::move_node(dir, project, &id, &target)
                }) {
                    Ok(()) => self.carrying = None,
                    Err(e) => {
                        self.pending = None;
                        self.message = Some(format!("moving {id}: {e} (esc cancels)"));
                    }
                }
            }
            _ => {}
        }
        None
    }

    /// The connection a conduit or end stands for.
    fn connection_at(&self, focus: &Focus) -> Option<Connection> {
        let (source, node, port, index) = match focus {
            Focus::Conduit(Link {
                source,
                output,
                target,
                port,
                index,
            }) => (
                Reference {
                    node: source.clone(),
                    port: output.clone(),
                },
                target.clone(),
                port.clone(),
                *index,
            ),
            Focus::In { node, row } => match &row.state {
                InState::Connected(source) => (
                    source.clone(),
                    node.clone(),
                    row.port.clone(),
                    row.index.unwrap_or(0),
                ),
                _ => return None,
            },
            Focus::Out { node, row } => {
                let user = row.user.as_ref()?;
                (
                    Reference {
                        node: node.clone(),
                        port: row.port.clone(),
                    },
                    user.node.clone(),
                    user.port.clone(),
                    user.index,
                )
            }
            Focus::Node(_) => return None,
        };
        Some(Connection {
            source,
            node,
            port,
            index,
        })
    }

    // --- history -----------------------------------------------------------------------

    /// `[` / `]` (§6.1): back or forward through flow changes, restoring the focus. A
    /// flow an edit has changed since is rebuilt around its focus's node.
    pub(super) fn flow_history(&mut self, back: bool) {
        let (from, to) = if back {
            (&mut self.flow_back, &mut self.flow_forward)
        } else {
            (&mut self.flow_forward, &mut self.flow_back)
        };
        let Some((mut flow, focus)) = from.pop() else {
            return;
        };
        if let Some(current) = self.flow_focus.clone() {
            to.push((self.flow.clone(), current));
        }
        let project = &self.project;
        if still_valid(project, &flow) {
            flow.extend(project);
        } else {
            let anchor = Some(focus.node().to_string())
                .filter(|n| project.get(n).is_some())
                .or_else(|| {
                    flow.nodes
                        .iter()
                        .find(|n| project.get(n).is_some())
                        .cloned()
                })
                .or_else(|| self.default_focus());
            let Some(anchor) = anchor else { return };
            flow = Flow::build(project, &anchor);
        }
        let focus = flow
            .locate(project, &focus)
            .unwrap_or_else(|| Focus::Node(flow.nodes.first().cloned().unwrap_or_default()));
        self.recentre = true;
        self.flow = flow;
        self.focus = Some(focus.node().to_string());
        self.flow_focus = Some(focus);
        self.refresh();
    }

    // --- helpers -----------------------------------------------------------------------

    fn is_multi(&self, node: &str, port: &str) -> bool {
        self.project
            .get(node)
            .and_then(|n| n.spec.as_ref()?.input(port))
            .is_some_and(|p| p.multi)
    }

    fn entries(&self, node: &str, port: &str) -> usize {
        self.project
            .get(node)
            .and_then(|n| n.inputs.get(port))
            .map_or(0, Vec::len)
    }
}

/// The flow after dissolving `id`, if its neighbours on the flow are now joined: the
/// flow without it, and the conduit that closed the gap (§6.7).
fn bridged(project: &Project, flow: &Flow, id: &str) -> Option<(Flow, Focus)> {
    let i = flow.position(id)?;
    let (up, down) = (flow.link_in(i)?, flow.link_out(i)?);
    let source = project
        .get(&down.target)?
        .inputs
        .get(&down.port)?
        .get(down.index)?;
    if source.node != up.source {
        return None;
    }
    let bridge = Link {
        source: up.source.clone(),
        output: source.port.clone(),
        target: down.target.clone(),
        port: down.port.clone(),
        index: down.index,
    };
    let mut flow = flow.clone();
    flow.nodes.remove(i);
    flow.links.splice(i - 1..=i, [bridge.clone()]);
    Some((flow, Focus::Conduit(bridge)))
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::KeyEvent;

    use super::super::tests::temp_project;
    use super::*;

    /// A temp copy of `examples/effects` in the flow view, focused on `id`.
    fn effects_on(id: &str) -> (tempfile::TempDir, App) {
        let (tmp, mut app) = temp_project("effects");
        app.flow_view = true;
        app.focus = Some(id.into());
        app.sync_flow();
        app.flow_back.clear();
        (tmp, app)
    }

    fn keys(app: &mut App, keys: &str) {
        for c in keys.chars() {
            let code = match c {
                '⏎' => KeyCode::Enter,
                '⎋' => KeyCode::Esc,
                c => KeyCode::Char(c),
            };
            app.handle_key(KeyEvent::from(code));
        }
    }

    /// Picks `choice` in the open picker by typing it.
    fn pick(app: &mut App, choice: &str) {
        assert!(matches!(app.mode, Mode::Pick { .. }), "{:?}", app.mode);
        keys(app, choice);
        keys(app, "⏎");
    }

    fn input(app: &App, node: &str, port: &str) -> Vec<String> {
        app.project.get(node).unwrap().inputs[port]
            .iter()
            .map(|r| r.short())
            .collect()
    }

    fn focused_end(app: &App) -> String {
        match app.flow_focus.as_ref().unwrap() {
            Focus::In { node, row } => format!("{node}.{}", row.label()),
            Focus::Out { node, row } => format!("{node}.{} out", row.port),
            Focus::Node(id) => id.clone(),
            Focus::Conduit(l) => format!("{}→{}.{}", l.source, l.target, l.port),
        }
    }

    #[test]
    fn c_on_an_off_flow_end_redraws_through_it() {
        let (_tmp, mut app) = effects_on("comp");
        keys(&mut app, "jj"); // comp.layers[0] ← logo_at, a ▣
        keys(&mut app, "c");
        pick(&mut app, "swatch_at");
        assert_eq!(input(&app, "comp", "layers")[0], "swatch_at");
        assert_eq!(focused_end(&app), "comp.layers[0]");
        assert_eq!(
            app.flow.nodes[app.flow.nodes.len() - 3..],
            ["swatch_at", "comp", "preview"]
        );
        assert_eq!(app.flow_back.len(), 1, "a flow change");
    }

    #[test]
    fn cancelling_a_picker_changes_nothing() {
        let (_tmp, mut app) = effects_on("comp");
        keys(&mut app, "jj");
        let before = (app.flow.clone(), app.flow_focus.clone());
        keys(&mut app, "c⎋");
        assert_eq!((app.flow.clone(), app.flow_focus.clone()), before);
        assert!(app.pending.is_none());
        assert!(app.flow_back.is_empty());
    }

    #[test]
    fn c_on_the_slot_appends_and_focuses_the_new_entry() {
        let (_tmp, mut app) = effects_on("comp");
        keys(&mut app, "kk"); // out, then the layers slot
        assert_eq!(focused_end(&app), "comp.layers");
        keys(&mut app, "c");
        pick(&mut app, "bg");
        assert_eq!(input(&app, "comp", "layers").len(), 4);
        assert_eq!(focused_end(&app), "comp.layers[3]");
        // The slot is one row below.
        keys(&mut app, "j");
        assert_eq!(focused_end(&app), "comp.layers");
    }

    #[test]
    fn an_array_comes_with_a_group_that_follows_its_count() {
        let (_tmp, mut app) = effects_on("comp");
        keys(&mut app, "jjf"); // logo_at → comp.layers[0]
        keys(&mut app, "i");
        pick(&mut app, "array");
        keys(&mut app, "⏎"); // the suggested id
        let array = input(&app, "array_1_group", "layers");
        assert_eq!(array, ["array_1.out1", "array_1.out2", "array_1.out3"]);
        assert_eq!(
            input(&app, "comp", "layers")[0],
            "array_1_group",
            "the group takes its place"
        );
        assert_eq!(input(&app, "array_1", "layer"), ["logo_at"]);
        assert!(app.project.is_valid(), "{:?}", app.project.errors);

        // A tweak on one item, then more columns: the new copies join the group after it.
        std::fs::write(
            app.dir.join("shade.toml"),
            "type = \"invert\"\n\n[inputs]\nlayer = \"array_1.out2\"\n",
        )
        .unwrap();
        let group = app.dir.join("array_1_group.toml");
        let text = std::fs::read_to_string(&group).unwrap();
        std::fs::write(&group, text.replace("\"array_1.out2\"", "\"shade\"")).unwrap();
        app.reload_from_disk();

        app.focus = Some("array_1".into());
        app.submit_prompt(
            Prompt::Param {
                name: "columns".into(),
            },
            "5",
        )
        .unwrap();
        assert_eq!(
            input(&app, "array_1_group", "layers"),
            [
                "array_1.out1",
                "shade",
                "array_1.out3",
                "array_1.out4",
                "array_1.out5"
            ]
        );
        assert!(app.project.is_valid(), "{:?}", app.project.errors);
        app.submit_prompt(
            Prompt::Param {
                name: "columns".into(),
            },
            "",
        )
        .unwrap(); // back to the default, 3
        assert_eq!(
            input(&app, "array_1_group", "layers"),
            ["array_1.out1", "shade", "array_1.out3"]
        );

        // Each edit is one undo; undoing the insert removes the array and its group.
        keys(&mut app, "uu");
        assert_eq!(input(&app, "array_1_group", "layers").len(), 3);
        keys(&mut app, "u");
        assert!(app.project.get("array_1").is_none() && app.project.get("array_1_group").is_none());
        assert_eq!(input(&app, "comp", "layers")[0], "logo_at");
    }

    #[test]
    fn i_into_the_flow_conduit_keeps_focus_on_the_end() {
        let (_tmp, mut app) = effects_on("comp");
        keys(&mut app, "jjf"); // logo_at → comp.layers[0], now in the flow
        keys(&mut app, "i");
        pick(&mut app, "invert");
        assert!(matches!(app.mode, Mode::Prompt { .. }));
        keys(&mut app, "⏎"); // the suggested id
        let new = input(&app, "comp", "layers")[0].clone();
        assert_eq!(input(&app, &new, "layer"), ["logo_at"]);
        assert_eq!(focused_end(&app), "comp.layers[0]");
        let n = app.flow.nodes.len();
        assert_eq!(
            app.flow.nodes[n - 4..],
            ["logo_at", new.as_str(), "comp", "preview"]
        );
        // h h reaches the new node.
        keys(&mut app, "hh");
        assert_eq!(focused_end(&app), new);
    }

    #[test]
    fn d_on_an_off_flow_entry_keeps_the_flow() {
        let (_tmp, mut app) = effects_on("comp");
        let flow = app.flow.clone();
        keys(&mut app, "jj"); // layers[0] ← logo_at
        keys(&mut app, "d");
        assert_eq!(input(&app, "comp", "layers"), ["swatch_at", "bar_inverted"]);
        // The row that took its place, renumbered.
        assert_eq!(focused_end(&app), "comp.layers[0]");
        assert_eq!(app.flow_status(), "disconnected comp.layers[0] ← logo_at");
        assert_eq!(app.flow, flow);
        assert!(app.flow_back.is_empty(), "not a flow change");
    }

    #[test]
    fn d_on_the_flow_conduit_rebuilds_upstream() {
        let (_tmp, mut app) = effects_on("comp");
        keys(&mut app, "f"); // nothing: comp isn't an end
        keys(&mut app, "jj");
        keys(&mut app, "f"); // logo → logo_tinted → logo_at → comp → preview
        assert_eq!(app.flow.nodes[0], "logo");
        keys(&mut app, "h"); // logo_at → comp.layers[0]
        keys(&mut app, "d");
        // comp stays; upstream rebuilt from its first connected input, base.
        assert_eq!(app.flow.nodes, ["bg", "comp", "preview"]);
        assert_eq!(focused_end(&app), "comp.layers[0]"); // swatch_at, renumbered
    }

    #[test]
    fn k_and_j_reorder_and_focus_follows() {
        let (_tmp, mut app) = effects_on("comp");
        keys(&mut app, "jjj"); // layers[1] ← swatch_at
        keys(&mut app, "J");
        assert_eq!(
            input(&app, "comp", "layers"),
            ["logo_at", "bar_inverted", "swatch_at"]
        );
        assert_eq!(focused_end(&app), "comp.layers[2]");
        assert_eq!(app.flow_status(), "moved comp.layers ← swatch_at to [2]");
        keys(&mut app, "KK");
        assert_eq!(focused_end(&app), "comp.layers[0]");
        assert_eq!(input(&app, "comp", "layers")[0], "swatch_at");
        assert!(app.flow_back.is_empty());
        // An in-flow entry's line moves with it.
        keys(&mut app, "f");
        keys(&mut app, "J");
        assert_eq!(focused_end(&app), "comp.layers[1]");
        let i = app.flow.position("comp").unwrap();
        assert_eq!(app.flow.link_in(i).unwrap().index, 1);
    }

    #[test]
    fn dissolve_focuses_the_bridging_conduit() {
        let (_tmp, mut app) = effects_on("logo_tinted");
        keys(&mut app, "d");
        assert!(matches!(app.mode, Mode::Confirm { .. }));
        keys(&mut app, "y");
        assert!(app.project.get("logo_tinted").is_none());
        assert_eq!(app.flow.nodes, ["logo", "logo_at", "comp", "preview"]);
        assert_eq!(focused_end(&app), "logo→logo_at.layer");
        assert!(app.flow_back.is_empty(), "not a flow change");
    }

    fn flow_on(example: &str, id: &str) -> (tempfile::TempDir, App) {
        let (tmp, mut app) = temp_project(example);
        app.flow_view = true;
        app.focus = Some(id.into());
        app.sync_flow();
        app.flow_back.clear();
        (tmp, app)
    }

    #[test]
    fn detach_lands_on_the_node_that_took_over_the_flow() {
        let (_tmp, mut app) = flow_on("array", "grid");
        let out = app
            .flow
            .links
            .iter()
            .find(|l| l.source == "grid")
            .unwrap()
            .output
            .clone();
        let bar: Vec<String> = app.bar().actions.iter().map(|a| a.text.clone()).collect();
        assert_eq!(bar.last().map(String::as_str), Some("(X)detach"), "{bar:?}");
        keys(&mut app, "X");
        let Mode::Confirm { question, .. } = &app.mode else {
            panic!("asks first: {:?}", app.flow_status());
        };
        assert!(
            question.starts_with("Detach `grid`? Makes 8 nodes; grid.out"),
            "{question}"
        );
        keys(&mut app, "y");
        assert!(app.project.get("grid").is_none());
        assert_eq!(focused_end(&app), format!("grid_{out}"));
        assert_eq!(app.flow_back.len(), 1, "a flow change");
        assert!(
            app.flow.nodes.contains(&"place".to_string()),
            "fed by what fed the array"
        );
        assert!(app.project.is_valid(), "{:?}", app.project.errors);

        keys(&mut app, "u");
        assert!(app.project.get("grid").is_some() && app.project.get("grid_out1").is_none());
    }

    #[test]
    fn detach_splits_text_into_text_and_transform_nodes() {
        let (_tmp, mut app) = flow_on("split_text", "word");
        keys(&mut app, "Xy");
        assert!(app.project.get("word").is_none());
        assert_eq!(
            app.project.get("word_char3_text").unwrap().kind.as_deref(),
            Some("text")
        );
        assert!(app.project.is_valid(), "{:?}", app.project.errors);
    }

    #[test]
    fn only_generators_detach() {
        let (_tmp, mut app) = flow_on("array", "place");
        keys(&mut app, "X");
        assert!(!matches!(app.mode, Mode::Confirm { .. }));
        assert_eq!(
            app.flow_status(),
            "can't detach place: place: `transform` nodes can't be detached"
        );
        // Cancelling leaves everything as it was.
        let (_tmp, mut app) = flow_on("array", "grid");
        keys(&mut app, "X⎋");
        assert!(app.project.get("grid").is_some());
        assert_eq!(focused_end(&app), "grid");
    }

    #[test]
    fn delete_hands_the_focus_downstream() {
        let (_tmp, mut app) = effects_on("logo_at");
        keys(&mut app, "Dy");
        assert!(app.project.get("logo_at").is_none());
        assert_eq!(focused_end(&app), "comp");
        assert_eq!(app.flow.nodes, ["bg", "comp", "preview"]);
        assert_eq!(app.flow_back.len(), 1);
        // A sink hands it upstream.
        keys(&mut app, "lll");
        assert_eq!(focused_end(&app), "preview");
        keys(&mut app, "Dy");
        assert_eq!(focused_end(&app), "comp");
    }

    #[test]
    fn move_picks_up_and_drops_into_a_conduit() {
        let (_tmp, mut app) = effects_on("preview");
        keys(&mut app, "m");
        assert_eq!(app.flow_status(), "can't move preview: it has no output");
        keys(&mut app, "/");
        pick(&mut app, "logo_tinted");
        keys(&mut app, "m");
        assert_eq!(app.carrying.as_deref(), Some("logo_tinted"));
        assert_eq!(app.bar().title, "Move Actions");
        keys(&mut app, "d"); // other verbs do nothing while moving
        assert!(app.project.get("logo_tinted").is_some());
        keys(&mut app, "llll"); // to comp
        keys(&mut app, "jjj"); // comp.layers[1] ← swatch_at
        keys(&mut app, "m");
        assert_eq!(app.carrying, None);
        assert_eq!(input(&app, "comp", "layers")[1], "logo_tinted");
        assert_eq!(input(&app, "logo_tinted", "layer"), ["swatch_at"]);
        assert_eq!(input(&app, "logo_at", "layer"), ["logo"]);
        assert_eq!(focused_end(&app), "logo_tinted");
    }

    #[test]
    fn slides_along_the_flow() {
        // logo → logo_tinted → logo_at → comp → preview
        let (_tmp, mut app) = effects_on("logo_tinted");
        assert!(app.bar().actions.iter().any(|a| a.text == "(L)slide"));
        keys(&mut app, "L");
        assert_eq!(app.flow_status(), "slid logo_tinted after logo_at");
        assert_eq!(
            app.flow.nodes,
            ["logo", "logo_at", "logo_tinted", "comp", "preview"]
        );
        assert_eq!(input(&app, "logo_at", "layer"), ["logo"]);
        assert_eq!(input(&app, "logo_tinted", "layer"), ["logo_at"]);
        assert_eq!(input(&app, "comp", "layers")[0], "logo_tinted");
        // Its linked param travels with it.
        assert_eq!(input(&app, "logo_tinted", "black"), ["brand.accent"]);
        assert_eq!(focused_end(&app), "logo_tinted");
        assert!(app.flow_back.is_empty(), "not a flow change");
        // One `u` puts it back.
        keys(&mut app, "u");
        assert_eq!(input(&app, "logo_tinted", "layer"), ["logo"]);
        assert_eq!(app.flow.nodes[..3], ["logo", "logo_tinted", "logo_at"]);
    }

    #[test]
    fn refused_slides_say_why() {
        let (_tmp, mut app) = effects_on("logo_at");
        // Past comp, logo_at would feed preview.image a layer.
        keys(&mut app, "L");
        assert_eq!(
            app.flow_status(),
            "can't slide logo_at after comp: preview.image takes an image, not a layer"
        );
        assert!(!app.bar().actions.iter().any(|a| a.text.contains("(L)")));
        let (_tmp, mut app) = effects_on("logo_tinted");
        keys(&mut app, "H");
        assert_eq!(
            app.flow_status(),
            "can't slide logo_tinted before logo: logo is at the start of the flow"
        );
        assert_eq!(input(&app, "logo_tinted", "layer"), ["logo"]);
    }

    #[test]
    fn history_goes_back_and_forward_through_flow_changes() {
        let (_tmp, mut app) = effects_on("comp");
        let first = (app.flow.clone(), app.flow_focus.clone());
        keys(&mut app, "jj");
        let end = app.flow_focus.clone();
        keys(&mut app, "f");
        let second = (app.flow.clone(), app.flow_focus.clone());
        keys(&mut app, "h"); // walking isn't a step
        keys(&mut app, "[");
        assert_eq!(app.flow, first.0);
        assert_eq!(app.flow_focus, end, "the focus where it was left");
        keys(&mut app, "[");
        assert_eq!(app.flow, first.0); // nothing further back
        keys(&mut app, "]");
        assert_eq!(app.flow, second.0);
    }

    #[test]
    fn rename_keeps_the_flow() {
        let (_tmp, mut app) = effects_on("comp");
        keys(&mut app, "r");
        assert!(matches!(app.mode, Mode::Prompt { .. }));
        keys(&mut app, "main⏎");
        assert_eq!(app.flow.nodes, ["bg", "main", "preview"]);
        assert_eq!(focused_end(&app), "main");
        assert!(app.flow_back.is_empty());
    }

    #[test]
    fn connect_to_from_an_unused_output() {
        let (_tmp, mut app) = effects_on("brand");
        keys(&mut app, "jjj"); // brand.sand, unused
        keys(&mut app, "c");
        pick(&mut app, "bg.color");
        assert_eq!(input(&app, "bg", "color"), ["brand.sand"]);
        assert_eq!(focused_end(&app), "brand.sand out");
        // The flow now runs through bg.
        assert_eq!(app.flow.nodes[..2], ["brand", "bg"]);
    }
}
