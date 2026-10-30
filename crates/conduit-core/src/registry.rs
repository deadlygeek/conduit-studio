use std::collections::BTreeMap;

use crate::node::{Node, NodeSpec};

/// Maps node kind names (the `type` field in node files) to implementations.
#[derive(Default)]
pub struct Registry {
    nodes: BTreeMap<&'static str, (Box<dyn Node>, NodeSpec)>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a node kind, adding inputs for its number params
    /// ([`NodeSpec::with_param_inputs`]). Panics on duplicate kinds or a bad spec — those
    /// are programming errors.
    pub fn register(&mut self, node: impl Node + 'static) {
        let spec = node.spec().with_param_inputs();
        let kind = spec.kind;
        let prev = self.nodes.insert(kind, (Box::new(node), spec));
        assert!(prev.is_none(), "node kind `{kind}` registered twice");
    }

    pub fn get(&self, kind: &str) -> Option<&dyn Node> {
        self.nodes.get(kind).map(|(n, _)| n.as_ref())
    }

    /// The spec of `kind`, computed once at registration.
    pub fn spec(&self, kind: &str) -> Option<&NodeSpec> {
        self.nodes.get(kind).map(|(_, s)| s)
    }

    /// All registered specs, sorted by kind.
    pub fn specs(&self) -> impl Iterator<Item = &NodeSpec> + '_ {
        self.nodes.values().map(|(_, s)| s)
    }
}
