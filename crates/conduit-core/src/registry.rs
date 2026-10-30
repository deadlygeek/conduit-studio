use std::collections::BTreeMap;

use crate::node::{Node, NodeSpec};

/// Maps node kind names (the `type` field in node files) to implementations.
#[derive(Default)]
pub struct Registry {
    nodes: BTreeMap<&'static str, Box<dyn Node>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a node kind. Panics on duplicate kinds — that's a programming error.
    pub fn register(&mut self, node: impl Node + 'static) {
        let kind = node.spec().kind;
        let prev = self.nodes.insert(kind, Box::new(node));
        assert!(prev.is_none(), "node kind `{kind}` registered twice");
    }

    pub fn get(&self, kind: &str) -> Option<&dyn Node> {
        self.nodes.get(kind).map(|n| n.as_ref())
    }

    /// All registered specs, sorted by kind.
    pub fn specs(&self) -> impl Iterator<Item = NodeSpec> + '_ {
        self.nodes.values().map(|n| n.spec())
    }
}
