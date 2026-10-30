//! Graph queries over a [`Project`] for editors: lineage trees, consumers, and which
//! connections are allowed.

use std::collections::BTreeSet;

use crate::graph::Reference;
use crate::project::Project;

/// One row of a lineage tree, in pre-order (a node's subtree follows it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeItem {
    /// 1 for the root's direct neighbours.
    pub depth: usize,
    pub node: String,
    /// The input port of the connection: upstream, the port on this item's parent that
    /// this node feeds; downstream, the port on this node that takes the parent.
    pub port: String,
    /// The output port on the upstream side of the connection.
    pub output: String,
    /// Already shown earlier in this tree (a diamond or cycle); not expanded again.
    pub repeat: bool,
    /// Referenced but no such node file exists.
    pub missing: bool,
}

impl Project {
    /// Everything `id` depends on, as a tree of its inputs. `max_depth` limits expansion.
    pub fn upstream_tree(&self, id: &str, max_depth: Option<usize>) -> Vec<TreeItem> {
        self.tree(id, max_depth, |p, node| {
            p.get(node)
                .map(|n| {
                    n.input_ports()
                        .into_iter()
                        .flat_map(|(port, refs)| {
                            refs.into_iter()
                                .map(move |r| (r.node, port.clone(), r.port))
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
    }

    /// Everything that depends on `id`, as a tree of its consumers.
    pub fn downstream_tree(&self, id: &str, max_depth: Option<usize>) -> Vec<TreeItem> {
        self.tree(id, max_depth, |p, node| {
            p.consumers(node)
                .iter()
                .map(|c| (c.node.clone(), c.port.clone(), c.output.clone()))
                .collect()
        })
    }

    /// Iterative pre-order walk (deep chains must not overflow the stack).
    /// `children` returns `(node, port, output)` for each neighbour.
    fn tree(
        &self,
        root: &str,
        max_depth: Option<usize>,
        children: impl Fn(&Project, &str) -> Vec<(String, String, String)>,
    ) -> Vec<TreeItem> {
        let mut seen = BTreeSet::from([root.to_string()]);
        let mut out = Vec::new();
        let push_children =
            |stack: &mut Vec<(usize, (String, String, String))>, node: &str, depth| {
                for child in children(self, node).into_iter().rev() {
                    stack.push((depth, child));
                }
            };
        let mut stack = Vec::new();
        push_children(&mut stack, root, 1);
        while let Some((depth, (node, port, output))) = stack.pop() {
            let missing = !self.nodes.contains_key(&node);
            let repeat = !missing && !seen.insert(node.clone());
            if !missing && !repeat && max_depth.is_none_or(|max| depth < max) {
                push_children(&mut stack, &node, depth + 1);
            }
            out.push(TreeItem {
                depth,
                node,
                port,
                output,
                repeat,
                missing,
            });
        }
        out
    }

    /// Every node that (transitively) consumes `id`'s outputs.
    pub fn descendants(&self, id: &str) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        let mut stack = vec![id.to_string()];
        while let Some(node) = stack.pop() {
            for c in self.consumers(&node) {
                if found.insert(c.node.clone()) {
                    stack.push(c.node.clone());
                }
            }
        }
        found
    }

    /// Whether feeding `source` into `target` would create a cycle.
    pub fn would_cycle(&self, source: &str, target: &str) -> bool {
        source == target || self.descendants(target).contains(source)
    }

    /// Outputs that may be connected to `node`'s input `port`: type-compatible and not
    /// creating a cycle. Empty if the node or port is unknown.
    pub fn compatible_sources(&self, node: &str, port: &str) -> Vec<Reference> {
        let Some(ty) = self
            .get(node)
            .and_then(|n| n.spec.as_ref())
            .and_then(|s| s.input(port))
            .map(|p| p.ty)
        else {
            return Vec::new();
        };
        let excluded = self.descendants(node);
        self.nodes
            .values()
            .filter(|n| n.id != node && !excluded.contains(&n.id))
            .flat_map(|n| {
                n.outputs()
                    .into_iter()
                    .filter(|(_, out_ty)| out_ty.coerces_to(ty))
                    .map(|(port, _)| Reference {
                        node: n.id.clone(),
                        port,
                    })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::project::inspect_sources;
    use crate::test_nodes::registry;

    fn project(files: &[(&str, String)]) -> Project {
        let sources = files
            .iter()
            .map(|(id, t)| (PathBuf::from(format!("{id}.toml")), t.clone()))
            .collect();
        inspect_sources(sources, &registry())
    }

    fn pass(input: &str) -> String {
        format!("type = \"pass\"\n[inputs]\nimage = \"{input}\"")
    }

    /// ```text
    /// src ─┬─ a ─ wa ─┐
    ///      └─ c ─ wc ─┴─ s   (s also references a missing node `ghost`)
    /// ```
    fn diamond() -> Project {
        project(&[
            ("src", "type = \"source\"".into()),
            ("a", pass("src")),
            ("c", pass("src")),
            ("wa", "type = \"wrap\"\n[inputs]\nimage = \"a\"".into()),
            ("wc", "type = \"wrap\"\n[inputs]\nimage = \"c\"".into()),
            (
                "s",
                "type = \"sink\"\n[inputs]\nitems = [\"wa\", \"wc\", \"ghost\"]".into(),
            ),
        ])
    }

    fn rows(items: &[TreeItem]) -> Vec<String> {
        items
            .iter()
            .map(|i| {
                let flag = if i.repeat {
                    " ↺"
                } else if i.missing {
                    " ?"
                } else {
                    ""
                };
                format!("{}{}{flag}", "  ".repeat(i.depth - 1), i.node)
            })
            .collect()
    }

    #[test]
    fn upstream_tree_marks_repeats_and_missing() {
        let p = diamond();
        assert_eq!(
            rows(&p.upstream_tree("s", None)),
            ["wa", "  a", "    src", "wc", "  c", "    src ↺", "ghost ?"]
        );
        let first = &p.upstream_tree("s", None)[0];
        assert_eq!(
            (first.port.as_str(), first.output.as_str()),
            ("items", "out")
        );
    }

    #[test]
    fn downstream_tree() {
        let p = diamond();
        assert_eq!(
            rows(&p.downstream_tree("src", None)),
            ["a", "  wa", "    s", "c", "  wc", "    s ↺"]
        );
        assert_eq!(rows(&p.downstream_tree("src", Some(1))), ["a", "c"]);
        assert!(p.downstream_tree("s", None).is_empty());
    }

    #[test]
    fn cycles_terminate() {
        let p = project(&[("a", pass("b")), ("b", pass("a"))]);
        assert_eq!(rows(&p.upstream_tree("a", None)), ["b", "  a ↺"]);
        assert!(p.would_cycle("a", "b"));
    }

    #[test]
    fn compatible_sources_respect_types_and_cycles() {
        let p = diamond();
        let names = |v: Vec<Reference>| v.into_iter().map(|r| r.to_string()).collect::<Vec<_>>();
        // `a.image` takes an image: wraps output layers, and c is fine; a's descendants
        // (wa, s) and a itself are excluded.
        assert_eq!(
            names(p.compatible_sources("a", "image")),
            ["c.out", "src.out"]
        );
        // `s.items` takes layers: images coerce too.
        assert_eq!(
            names(p.compatible_sources("s", "items")),
            ["a.out", "c.out", "src.out", "wa.out", "wc.out"]
        );
        assert!(p.compatible_sources("s", "nope").is_empty());
        assert!(p.compatible_sources("ghost", "image").is_empty());
        assert!(p.would_cycle("s", "src"));
        assert!(!p.would_cycle("src", "s"));
    }

    #[test]
    fn large_graph() {
        // A 2000-node chain plus a sink fed by 500 wraps of the chain's start.
        let mut files: Vec<(String, String)> = vec![("n0".into(), "type = \"source\"".into())];
        for i in 1..2000 {
            files.push((format!("n{i}"), pass(&format!("n{}", i - 1))));
        }
        let mut items = Vec::new();
        for i in 0..500 {
            files.push((
                format!("w{i}"),
                "type = \"wrap\"\n[inputs]\nimage = \"n0\"".into(),
            ));
            items.push(format!("\"w{i}\""));
        }
        files.push((
            "sink".into(),
            format!("type = \"sink\"\n[inputs]\nitems = [{}]", items.join(", ")),
        ));
        let files: Vec<(&str, String)> =
            files.iter().map(|(a, b)| (a.as_str(), b.clone())).collect();

        let start = Instant::now();
        let p = project(&files);
        assert!(p.is_valid(), "{:?}", p.errors.first());
        assert_eq!(p.upstream_tree("n1999", None).len(), 1999);
        assert_eq!(p.downstream_tree("n0", None).len(), 1999 + 500 + 500);
        assert_eq!(p.upstream_tree("sink", Some(1)).len(), 500);
        assert_eq!(p.compatible_sources("n5", "image").len(), 5); // n0..n4
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "took {:?}",
            start.elapsed()
        );
    }
}
