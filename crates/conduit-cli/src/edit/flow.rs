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
    #[allow(dead_code)] // for `f` on a `▣` end, wired up with the flow keys
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
    #[allow(dead_code)] // for `f` on a `▣` end, wired up with the flow keys
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
