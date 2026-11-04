//! The flow model (spec §3.1, §4): one path through the graph, from a source to a sink,
//! and the port rows of each node box on it. Pure data, independent of drawing.

use std::collections::BTreeSet;

use conduit_core::{Consumer, Project, ProjectNode, Reference};

/// One conduit drawn in the flow: `source.output` feeds `target.port[index]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub source: String,
    pub output: String,
    pub target: String,
    pub port: String,
    /// Position in the target's input list (0 for a single input).
    pub index: usize,
}

/// One flow: `nodes` left to right; `links[i]` joins `nodes[i]` to `nodes[i + 1]`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flow {
    pub nodes: Vec<String>,
    pub links: Vec<Link>,
}

impl Flow {
    /// The flow around `anchor` (§4.2): upstream through each node's first connected
    /// input, downstream through the first user of each node's first used output.
    /// Empty if `anchor` isn't in the project.
    pub fn build(project: &Project, anchor: &str) -> Self {
        if project.get(anchor).is_none() {
            return Self::default();
        }
        let mut flow = Self {
            nodes: vec![anchor.to_string()],
            links: Vec::new(),
        };
        flow.extend_upstream(project);
        flow.extend_downstream(project);
        flow
    }

    /// Extends both ends after an edit that kept the flow's path, e.g. when the last
    /// node gained a user.
    pub fn extend(&mut self, project: &Project) {
        self.extend_upstream(project);
        self.extend_downstream(project);
    }

    /// Keeps `nodes[i..]` and rebuilds the upstream side from `nodes[i]`'s first connected
    /// input (§6.7: `d` on the flow's conduit).
    pub fn rebuild_upstream(&mut self, project: &Project, i: usize) {
        self.nodes.drain(..i.min(self.nodes.len()));
        self.links.drain(..i.min(self.links.len()));
        if !self.nodes.is_empty() {
            self.extend_upstream(project);
        }
    }

    /// Where `node` is in the flow.
    pub fn position(&self, node: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n == node)
    }

    /// The link into `nodes[i]`, if it isn't the first node.
    pub fn link_in(&self, i: usize) -> Option<&Link> {
        i.checked_sub(1).and_then(|j| self.links.get(j))
    }

    /// The link out of `nodes[i]`, if it isn't the last node.
    pub fn link_out(&self, i: usize) -> Option<&Link> {
        self.links.get(i)
    }

    /// Redraws the upstream side through input `port[index]` of `nodes[i]` (§4.4): that
    /// node and everything downstream of it stay. Does nothing if the input isn't
    /// connected.
    pub fn through_input(&mut self, project: &Project, i: usize, port: &str, index: usize) {
        let Some(target) = self.nodes.get(i).cloned() else {
            return;
        };
        let Some(source) = project
            .get(&target)
            .and_then(|n| n.inputs.get(port))
            .and_then(|refs| refs.get(index))
            .cloned()
        else {
            return;
        };
        self.nodes.drain(..i);
        self.links.drain(..i);
        self.nodes.insert(0, source.node.clone());
        self.links.insert(
            0,
            Link {
                source: source.node,
                output: source.port,
                target,
                port: port.to_string(),
                index,
            },
        );
        self.extend_upstream(project);
    }

    /// Redraws the downstream side through `user` of `nodes[i]`'s output (§4.4): that
    /// node and everything upstream of it stay.
    pub fn through_output(&mut self, project: &Project, i: usize, user: &Consumer) {
        let Some(source) = self.nodes.get(i).cloned() else {
            return;
        };
        if project.get(&user.node).is_none() {
            return;
        }
        self.nodes.truncate(i + 1);
        self.links.truncate(i);
        self.nodes.push(user.node.clone());
        self.links.push(link_to(&source, user));
        self.extend_downstream(project);
    }

    /// Adds nodes on the left until a source, or a node already in the flow (only in a
    /// project with a cycle).
    fn extend_upstream(&mut self, project: &Project) {
        let mut seen: BTreeSet<String> = self.nodes.iter().cloned().collect();
        while let Some(node) = project.get(&self.nodes[0]) {
            let Some((port, index, source)) = first_source(node) else {
                break;
            };
            if project.get(&source.node).is_none() || !seen.insert(source.node.clone()) {
                break;
            }
            self.links.insert(
                0,
                Link {
                    source: source.node.clone(),
                    output: source.port.clone(),
                    target: node.id.clone(),
                    port,
                    index,
                },
            );
            self.nodes.insert(0, source.node);
        }
    }

    /// Adds nodes on the right until a sink, or a node already in the flow.
    fn extend_downstream(&mut self, project: &Project) {
        let mut seen: BTreeSet<String> = self.nodes.iter().cloned().collect();
        loop {
            let last = self.nodes.last().unwrap().clone();
            let Some(node) = project.get(&last) else {
                break;
            };
            let Some(user) = first_user(project, node) else {
                break;
            };
            if project.get(&user.node).is_none() || !seen.insert(user.node.clone()) {
                break;
            }
            self.links.push(link_to(&last, user));
            self.nodes.push(user.node.clone());
        }
    }
}

fn link_to(source: &str, user: &Consumer) -> Link {
    Link {
        source: source.to_string(),
        output: user.output.clone(),
        target: user.node.clone(),
        port: user.port.clone(),
        index: user.index,
    }
}

/// The node's first connected input in row order (§4.2): `(port, index, source)`.
fn first_source(node: &ProjectNode) -> Option<(String, usize, Reference)> {
    input_rows(node)
        .into_iter()
        .find_map(|row| match row.state {
            InState::Connected(source) => Some((row.port, row.index.unwrap_or(0), source)),
            _ => None,
        })
}

/// The first user of the node's first output that has one, outputs in port order and
/// users in id order (§4.2).
fn first_user<'a>(project: &'a Project, node: &ProjectNode) -> Option<&'a Consumer> {
    let users = project.consumers(&node.id);
    node.outputs()
        .iter()
        .find_map(|(port, _)| users.iter().find(|c| &c.output == port))
        // A reference to an output the node doesn't have (an error) still draws.
        .or_else(|| users.first())
}

/// The single thing the keyboard acts on in the flow view (spec §2, §5.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Focus {
    Node(String),
    Conduit(Link),
    /// An input row (a conduit end, an unconnected input or a list's slot).
    In {
        node: String,
        row: InRow,
    },
    /// An output row.
    Out {
        node: String,
        row: OutRow,
    },
}

impl Focus {
    /// The node global keys act on (§6.2): the node itself, the node an end belongs to,
    /// a conduit's downstream node.
    pub fn node(&self) -> &str {
        match self {
            Focus::Node(id) | Focus::In { node: id, .. } | Focus::Out { node: id, .. } => id,
            Focus::Conduit(link) => &link.target,
        }
    }
}

/// A focus move (§6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    /// `h`
    Upstream,
    /// `l`
    Downstream,
    /// `j`
    Next,
    /// `k`
    Previous,
    /// `esc`
    ToNode,
    /// `g`
    First,
    /// `G`
    Last,
}

/// Whether input `row` of `nodes[i]` is the flow's conduit into it.
pub fn is_flow_input(flow: &Flow, i: usize, row: &InRow) -> bool {
    matches!(row.state, InState::Connected(_))
        && flow
            .link_in(i)
            .is_some_and(|l| l.port == row.port && l.index == row.index.unwrap_or(0))
}

/// Whether output `row` of `nodes[i]` is the flow's conduit out of it.
pub fn is_flow_output(flow: &Flow, i: usize, row: &OutRow) -> bool {
    let (Some(link), Some(user)) = (flow.link_out(i), &row.user) else {
        return false;
    };
    link.output == row.port
        && link.target == user.node
        && link.port == user.port
        && link.index == user.index
}

/// Whether two input rows are the same conduit end: same port and list position.
pub fn same_input(a: &InRow, b: &InRow) -> bool {
    a.port == b.port && a.index == b.index
}

/// Whether two output rows are the same conduit end: same output and user.
pub fn same_output(a: &OutRow, b: &OutRow) -> bool {
    a.port == b.port && a.user == b.user
}

impl Flow {
    /// The focus, with its row refreshed from the project, if it's still on this flow.
    pub fn locate(&self, project: &Project, focus: &Focus) -> Option<Focus> {
        let node = project.get(focus.node())?;
        self.position(&node.id)?;
        match focus {
            Focus::Node(id) => Some(Focus::Node(id.clone())),
            Focus::Conduit(link) => self.links.contains(link).then(|| focus.clone()),
            Focus::In { node: id, row } => input_rows(node)
                .into_iter()
                .find(|r| same_input(r, row))
                .map(|row| Focus::In {
                    node: id.clone(),
                    row,
                }),
            Focus::Out { node: id, row } => output_rows(project, node)
                .into_iter()
                .find(|r| same_output(r, row))
                .map(|row| Focus::Out {
                    node: id.clone(),
                    row,
                }),
        }
    }

    /// Where `mv` takes the focus, or `None` if it goes nowhere (§6.1).
    pub fn step(&self, project: &Project, focus: &Focus, mv: Move) -> Option<Focus> {
        let i = self.position(focus.node())?;
        let node = project.get(focus.node())?;
        let node_focus = |i: usize| Focus::Node(self.nodes[i].clone());
        let ins = input_rows(node);
        let outs = output_rows(project, node);
        // Rows top to bottom: inputs, then outputs.
        let row_focus = |r: usize| -> Focus {
            if r < ins.len() {
                Focus::In {
                    node: node.id.clone(),
                    row: ins[r].clone(),
                }
            } else {
                Focus::Out {
                    node: node.id.clone(),
                    row: outs[r - ins.len()].clone(),
                }
            }
        };
        let rows = ins.len() + outs.len();
        let row_index = match focus {
            Focus::In { row, .. } => ins.iter().position(|r| same_input(r, row)),
            Focus::Out { row, .. } => outs
                .iter()
                .position(|r| same_output(r, row))
                .map(|r| r + ins.len()),
            _ => None,
        };
        match (focus, mv) {
            (_, Move::First) => Some(node_focus(0)),
            (_, Move::Last) => Some(node_focus(self.nodes.len() - 1)),
            (Focus::Conduit(_), Move::ToNode) => Some(node_focus(i)),
            (Focus::Conduit(_), Move::Upstream) => Some(node_focus(i - 1)),
            (Focus::Conduit(_), Move::Downstream) => Some(node_focus(i)),
            (Focus::Conduit(_), _) => None,
            (Focus::Node(_), Move::Upstream) => self.link_in(i).cloned().map(Focus::Conduit),
            (Focus::Node(_), Move::Downstream) => self.link_out(i).cloned().map(Focus::Conduit),
            (Focus::Node(_), Move::Next) => (rows > 0).then(|| row_focus(0)),
            (Focus::Node(_), Move::Previous) => (rows > 0).then(|| row_focus(rows - 1)),
            (Focus::Node(_), Move::ToNode) => None,
            (_, Move::ToNode) => Some(node_focus(i)),
            (Focus::In { row, .. }, Move::Upstream) => is_flow_input(self, i, row)
                .then(|| self.link_in(i).cloned().map(Focus::Conduit))
                .flatten(),
            (Focus::In { .. }, Move::Downstream) => Some(node_focus(i)),
            (Focus::Out { row, .. }, Move::Downstream) => is_flow_output(self, i, row)
                .then(|| self.link_out(i).cloned().map(Focus::Conduit))
                .flatten(),
            (Focus::Out { .. }, Move::Upstream) => Some(node_focus(i)),
            (_, Move::Next) => Some(match row_index {
                Some(r) if r + 1 < rows => row_focus(r + 1),
                _ => node_focus(i),
            }),
            (_, Move::Previous) => Some(match row_index {
                Some(r) if r > 0 => row_focus(r - 1),
                _ => node_focus(i),
            }),
        }
    }

    /// `f` on a `▣` end (§4.4): brings its conduit into the flow, redrawing only the side
    /// it points to. `None` if the focus isn't a `▣` end.
    pub fn bring_in(&mut self, project: &Project, focus: &Focus) -> Option<()> {
        let i = self.position(focus.node())?;
        match focus {
            Focus::In { row, .. } => {
                if !matches!(row.state, InState::Connected(_)) || is_flow_input(self, i, row) {
                    return None;
                }
                self.through_input(project, i, &row.port, row.index.unwrap_or(0));
            }
            Focus::Out { row, .. } => {
                let user = row.user.as_ref()?;
                if is_flow_output(self, i, row) {
                    return None;
                }
                self.through_output(project, i, user);
            }
            _ => return None,
        }
        Some(())
    }
}

/// What an input row is connected to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InState {
    Connected(Reference),
    /// A list input's empty slot, after its entries (§3.1).
    Slot,
    /// Unconnected and optional.
    Optional,
    /// Unconnected and required (an error).
    Missing,
}

/// One input row of a node box: one conduit end (§3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InRow {
    pub port: String,
    /// The entry's position in a list input; `None` for a single input or a slot.
    pub index: Option<usize>,
    pub state: InState,
}

impl InRow {
    /// `layers[1]` for a list entry, else the port name.
    pub fn label(&self) -> String {
        match self.index {
            Some(i) => format!("{}[{i}]", self.port),
            None => self.port.clone(),
        }
    }
}

/// One output row of a node box: an output and one of its users, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutRow {
    pub port: String,
    pub user: Option<Consumer>,
}

/// Input rows in port order (plain inputs before linked params, as the spec lists them):
/// one per connection, a slot after a list's entries, one for an unconnected input; an
/// unlinked param has none (§3.1).
pub fn input_rows(node: &ProjectNode) -> Vec<InRow> {
    let mut rows = Vec::new();
    for (port, refs) in node.input_ports() {
        let spec = node.spec.as_ref().and_then(|s| s.input(&port));
        let multi = spec.is_some_and(|p| p.multi);
        let is_param = spec.is_some_and(|p| p.param);
        for (i, source) in refs.iter().enumerate() {
            rows.push(InRow {
                port: port.clone(),
                index: (multi || refs.len() > 1).then_some(i),
                state: InState::Connected(source.clone()),
            });
        }
        let state = if multi {
            InState::Slot
        } else if !refs.is_empty() || is_param {
            continue;
        } else if spec.is_some_and(|p| p.optional) {
            InState::Optional
        } else {
            InState::Missing
        };
        rows.push(InRow {
            port,
            index: None,
            state,
        });
    }
    rows
}

/// Output rows: one per user of each output, users in id order; one row for an output
/// nobody uses (§3.1).
pub fn output_rows(project: &Project, node: &ProjectNode) -> Vec<OutRow> {
    let users = project.consumers(&node.id);
    let mut rows = Vec::new();
    let outputs = node.outputs();
    for (port, _) in &outputs {
        let mut any = false;
        for user in users.iter().filter(|c| &c.output == port) {
            rows.push(OutRow {
                port: port.clone(),
                user: Some(user.clone()),
            });
            any = true;
        }
        if !any {
            rows.push(OutRow {
                port: port.clone(),
                user: None,
            });
        }
    }
    // References to outputs the node doesn't have (an error) still get a row each.
    for user in users {
        if !outputs.iter().any(|(p, _)| p == &user.output) {
            rows.push(OutRow {
                port: user.output.clone(),
                user: Some(user.clone()),
            });
        }
    }
    rows
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;

    use conduit_core::inspect;

    use super::*;

    pub(crate) fn example(name: &str) -> Project {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples")
            .join(name);
        inspect(&dir, &conduit_nodes::registry()).unwrap()
    }

    #[test]
    fn builds_from_the_sink_through_first_inputs() {
        let project = example("effects");
        let flow = Flow::build(&project, "preview");
        assert_eq!(flow.nodes, ["bg", "comp", "preview"]);
        assert_eq!(
            flow.links[0],
            Link {
                source: "bg".into(),
                output: "out".into(),
                target: "comp".into(),
                port: "base".into(),
                index: 0,
            }
        );
    }

    #[test]
    fn builds_both_ways_from_the_middle() {
        let project = example("effects");
        let flow = Flow::build(&project, "logo_tinted");
        assert_eq!(
            flow.nodes,
            ["logo", "logo_tinted", "logo_at", "comp", "preview"]
        );
        assert_eq!(flow.links[2].port, "layers");
        assert_eq!(flow.links[2].index, 0);
    }

    #[test]
    fn downstream_skips_unused_outputs() {
        // brand.primary is the first colour used (by bar); then on to the sink.
        let project = example("effects");
        let flow = Flow::build(&project, "brand");
        assert_eq!(flow.nodes[0], "brand");
        assert_eq!(flow.links[0].output, "primary");
        assert_eq!(flow.nodes.last().unwrap(), "preview");
    }

    #[test]
    fn redraws_one_side() {
        let project = example("effects");
        let mut flow = Flow::build(&project, "preview");
        flow.through_input(&project, 1, "layers", 1);
        assert_eq!(flow.nodes[flow.nodes.len() - 2..], ["comp", "preview"]);
        assert_eq!(
            flow.link_in(flow.position("comp").unwrap()).unwrap().index,
            1
        );
        assert!(flow.nodes.contains(&"swatch_at".to_string()));

        let brand = project.get("brand").unwrap();
        let user = output_rows(&project, brand)
            .into_iter()
            .filter_map(|r| r.user)
            .find(|u| u.node == "logo_tinted")
            .unwrap();
        let mut flow = Flow::build(&project, "brand");
        flow.through_output(&project, 0, &user);
        assert_eq!(flow.nodes[..3], ["brand", "logo_tinted", "logo_at"]);
    }

    fn node(id: &str) -> Focus {
        Focus::Node(id.into())
    }

    #[test]
    fn walks_the_flow() {
        let project = example("effects");
        let flow = Flow::build(&project, "preview");
        let step = |f: &Focus, mv| flow.step(&project, f, mv);

        // h: node → conduit → node; l back.
        let conduit = step(&node("preview"), Move::Upstream).unwrap();
        assert!(matches!(&conduit, Focus::Conduit(l) if l.source == "comp"));
        assert_eq!(step(&conduit, Move::Upstream), Some(node("comp")));
        assert_eq!(step(&conduit, Move::Downstream), Some(node("preview")));
        assert_eq!(step(&node("bg"), Move::Upstream), None);
        assert_eq!(step(&conduit, Move::Next), None);

        // j / k through comp's rows, and past the ends back to the node.
        let first = step(&node("comp"), Move::Next).unwrap();
        assert!(matches!(&first, Focus::In { row, .. } if row.port == "base"));
        let last = step(&node("comp"), Move::Previous).unwrap();
        assert!(matches!(&last, Focus::Out { row, .. } if row.port == "out"));
        assert_eq!(step(&last, Move::Next), Some(node("comp")));
        assert_eq!(step(&first, Move::Previous), Some(node("comp")));
        let layer0 = step(&first, Move::Next).unwrap();

        // h on the in-flow input goes onto its conduit; on a `▣` it does nothing.
        assert!(matches!(
            step(&first, Move::Upstream),
            Some(Focus::Conduit(_))
        ));
        assert_eq!(step(&layer0, Move::Upstream), None);
        assert_eq!(step(&layer0, Move::Downstream), Some(node("comp")));
        assert_eq!(step(&layer0, Move::ToNode), Some(node("comp")));
        assert_eq!(step(&layer0, Move::Last), Some(node("preview")));
        assert_eq!(step(&layer0, Move::First), Some(node("bg")));
    }

    #[test]
    fn f_brings_an_end_into_the_flow() {
        let project = example("effects");
        let mut flow = Flow::build(&project, "preview");
        let base = flow.step(&project, &node("comp"), Move::Next).unwrap();
        let layer1 = flow
            .step(
                &project,
                &flow.step(&project, &base, Move::Next).unwrap(),
                Move::Next,
            )
            .unwrap();
        assert_eq!(flow.bring_in(&project, &base), None); // already in the flow
        flow.bring_in(&project, &layer1).unwrap();
        // swatch takes its colour from brand, so the new upstream side starts there.
        assert_eq!(
            flow.nodes,
            [
                "brand",
                "swatch",
                "swatch_shifted",
                "swatch_at",
                "comp",
                "preview"
            ]
        );
        // The focus is still on the flow, now drawn.
        let layer1 = flow.locate(&project, &layer1).unwrap();
        let Focus::In { row, .. } = &layer1 else {
            panic!()
        };
        assert!(is_flow_input(&flow, 4, row));
    }

    #[test]
    fn rows_follow_the_spec() {
        let project = example("effects");
        let comp = project.get("comp").unwrap();
        let labels: Vec<String> = input_rows(comp).iter().map(InRow::label).collect();
        assert_eq!(
            labels,
            ["base", "layers[0]", "layers[1]", "layers[2]", "layers"]
        );
        assert_eq!(input_rows(comp)[4].state, InState::Slot);

        // tint: a plain input and a linked param; unlinked params have no row.
        let tint = project.get("logo_tinted").unwrap();
        let labels: Vec<String> = input_rows(tint).iter().map(InRow::label).collect();
        assert_eq!(labels, ["layer", "black"]);

        // A palette: one row per user of each colour, one for each unused colour.
        let brand = project.get("brand").unwrap();
        let rows = output_rows(&project, brand);
        assert!(rows.iter().any(|r| r.port == "sand" && r.user.is_none()));
        assert_eq!(rows.len(), project.consumers("brand").len() + 2);
    }
}
