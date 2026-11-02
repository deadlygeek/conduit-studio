//! Evaluating a loaded graph, skipping nodes whose inputs and params haven't changed.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::graph::{Graph, GraphNode, Reference};
use crate::node::{EvalCtx, GuiHost, Inputs, Node, NodeSpec, ParamKind};
use crate::registry::Registry;
use crate::value::Value;

/// Every output produced during an evaluation, keyed by `node.port`.
pub type Values = HashMap<Reference, Value>;

#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    #[error("{node}: {message}")]
    Node { node: String, message: String },
    #[error("{node}: node did not produce output `{port}` as {expected}")]
    BadOutput {
        node: String,
        port: String,
        expected: String,
    },
}

/// What an evaluation did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EvalStats {
    pub nodes: usize,
    /// Nodes that actually ran; the rest reused their previous outputs.
    pub evaluated: usize,
}

/// Evaluates every node in order, once. The graph must have been loaded with the same
/// registry.
pub fn evaluate(
    graph: &Graph,
    registry: &Registry,
    gui: &mut dyn GuiHost,
) -> Result<Values, EvalError> {
    Evaluator::default()
        .evaluate(graph, registry, gui)
        .map(|(values, _)| values)
}

/// Evaluates graphs repeatedly, remembering each node's outputs. A node only runs again
/// when its kind, params, the files it reads or (transitively) anything upstream of it
/// changed. GUI nodes
/// always run, so their windows get a fresh frame (e.g. after showing an error).
#[derive(Default)]
pub struct Evaluator {
    /// Node id → fingerprint of what produced its outputs, and the outputs.
    cache: HashMap<String, (u64, Vec<(Reference, Value)>)>,
}

impl Evaluator {
    pub fn evaluate(
        &mut self,
        graph: &Graph,
        registry: &Registry,
        gui: &mut dyn GuiHost,
    ) -> Result<(Values, EvalStats), EvalError> {
        let mut values = Values::new();
        let mut fingerprints: HashMap<&str, u64> = HashMap::new();
        let mut cache = HashMap::with_capacity(graph.nodes.len());
        let mut stats = EvalStats {
            nodes: graph.nodes.len(),
            evaluated: 0,
        };
        for node in &graph.nodes {
            let spec = registry
                .spec(&node.kind)
                .expect("graph loaded with this registry");
            let node_impl = registry
                .get(&node.kind)
                .expect("graph loaded with this registry");
            let files = node_impl.files(&node.params);
            let fingerprint = fingerprint(node, &graph.root, &files, &fingerprints);
            fingerprints.insert(&node.id, fingerprint);

            let outputs = match self.cache.remove(&node.id) {
                Some((fp, outputs)) if fp == fingerprint && !spec.gui => outputs,
                _ => {
                    stats.evaluated += 1;
                    eval_node(node, spec, node_impl, &graph.root, &values, gui)?
                }
            };
            for (r, v) in &outputs {
                values.insert(r.clone(), v.clone());
            }
            cache.insert(node.id.clone(), (fingerprint, outputs));
        }
        // Nodes that are gone (or failed) are dropped from the cache.
        self.cache = cache;
        Ok((values, stats))
    }
}

/// The node's params with linked inputs written over them: colors as hex, numbers checked
/// against the param's range (ints are rounded). Fonts are left to the node.
fn linked_params(
    node: &GraphNode,
    spec: &NodeSpec,
    inputs: &HashMap<String, Vec<Value>>,
) -> Result<toml::Table, String> {
    let mut params = node.params.clone();
    for port in spec.inputs.iter().filter(|p| p.param) {
        let Some(input) = inputs.get(port.name).and_then(|v| v.first()) else {
            continue;
        };
        let source = &node.inputs[port.name][0];
        let param = spec
            .param(port.name)
            .expect("param inputs come from params");
        let n = match input {
            Value::Number(n) => n,
            Value::Color(c) => {
                let hex = crate::color::to_hex(*c);
                params.insert(port.name.to_string(), toml::Value::String(hex));
                continue;
            }
            Value::Font(_) => continue,
            _ => unreachable!("param inputs are numbers, colors or fonts"),
        };
        let value = match param.kind {
            ParamKind::Int { min, max } => {
                let v = n.round();
                // `as` saturates, so check the range on the float first.
                if !(v >= min as f64 && v <= max as f64) {
                    return Err(format!(
                        "`{}` from `{source}` must be between {min} and {max}, got {n}",
                        port.name
                    ));
                }
                toml::Value::Integer(v as i64)
            }
            ParamKind::Float { min, max } => {
                if !(*n >= min && *n <= max) {
                    return Err(format!(
                        "`{}` from `{source}` must be between {min} and {max}, got {n}",
                        port.name
                    ));
                }
                toml::Value::Float(*n)
            }
            _ => unreachable!("only number params get inputs"),
        };
        params.insert(port.name.to_string(), value);
    }
    Ok(params)
}

/// Identifies everything a node's outputs depend on: its kind, its params, the size and
/// modification time of the files it reads, and the fingerprints of its sources (which are
/// computed first, in graph order).
fn fingerprint(
    node: &GraphNode,
    root: &Path,
    files: &[PathBuf],
    fingerprints: &HashMap<&str, u64>,
) -> u64 {
    let mut h = DefaultHasher::new();
    node.kind.hash(&mut h);
    node.params.to_string().hash(&mut h);
    for file in files {
        file.hash(&mut h);
        std::fs::metadata(root.join(file))
            .ok()
            .map(|m| (m.len(), m.modified().ok()))
            .hash(&mut h);
    }
    for (port, refs) in &node.inputs {
        port.hash(&mut h);
        for r in refs {
            fingerprints.get(r.node.as_str()).hash(&mut h);
            r.port.hash(&mut h);
        }
    }
    h.finish()
}

fn eval_node(
    node: &GraphNode,
    spec: &NodeSpec,
    node_impl: &dyn Node,
    root: &Path,
    values: &Values,
    gui: &mut dyn GuiHost,
) -> Result<Vec<(Reference, Value)>, EvalError> {
    let mut inputs = HashMap::new();
    for (port, refs) in &node.inputs {
        let ty = spec.input(port).expect("port validated at load").ty;
        let port_values = refs
            .iter()
            .map(|r| {
                let v = values.get(r).cloned().expect("sources evaluated first");
                v.coerce(ty).expect("types validated at load")
            })
            .collect();
        inputs.insert(port.clone(), port_values);
    }

    let node_error = |message: String| EvalError::Node {
        node: node.id.clone(),
        message,
    };
    let params = linked_params(node, spec, &inputs).map_err(node_error)?;
    let mut ctx = EvalCtx {
        node_id: &node.id,
        root,
        gui,
    };
    let outputs = node_impl
        .eval(&Inputs::new(inputs), &params, &mut ctx)
        .map_err(|e| node_error(e.0))?;

    let mut outputs = outputs.values;
    spec.outputs_for(&node.params)
        .into_iter()
        .map(|(name, ty)| match outputs.remove(&name) {
            Some(v) if v.ty() == ty => Ok((
                Reference {
                    node: node.id.clone(),
                    port: name,
                },
                v,
            )),
            _ => Err(EvalError::BadOutput {
                node: node.id.clone(),
                port: name,
                expected: ty.to_string(),
            }),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::RgbaImage;
    use crate::graph::from_sources;
    use crate::test_nodes::{registry, sources};

    #[derive(Default)]
    struct Recorder(Vec<(String, u32)>);

    impl GuiHost for Recorder {
        fn show(&mut self, node_id: &str, image: Arc<RgbaImage>) {
            self.0.push((node_id.to_string(), image.width()));
        }
    }

    #[test]
    fn evaluates_and_coerces_inputs() {
        let reg = registry();
        let files = [
            ("img", "type = \"source\"\n[params]\nsize = 7"),
            ("w", "type = \"wrap\"\n[inputs]\nimage = \"img\""),
            // `items` takes layers: one real layer, one image coerced to a layer.
            ("s", "type = \"sink\"\n[inputs]\nitems = [\"w\", \"img\"]"),
        ];
        let graph = graph(&files);

        let mut gui = Recorder::default();
        let values = evaluate(&graph, &reg, &mut gui).unwrap();

        let w_out = Reference::parse("w").unwrap();
        assert!(matches!(&values[&w_out], Value::Layer(l) if l.image.width() == 7));
        assert_eq!(gui.0, [("s".to_string(), 7), ("s".to_string(), 7)]);
    }

    fn graph(files: &[(&str, &str)]) -> Graph {
        from_sources(sources(files), &registry()).unwrap()
    }

    #[test]
    fn reruns_only_changed_nodes() {
        let reg = registry();
        let mut files = vec![
            ("a", "type = \"source\"\n[params]\nsize = 2"),
            ("b", "type = \"source\"\n[params]\nsize = 3"),
            ("wa", "type = \"wrap\"\n[inputs]\nimage = \"a\""),
            ("wb", "type = \"wrap\"\n[inputs]\nimage = \"b\""),
            ("s", "type = \"sink\"\n[inputs]\nitems = [\"wa\", \"wb\"]"),
        ];
        let mut evaluator = Evaluator::default();
        let mut run = |files: &[(&str, &str)]| {
            let mut gui = Recorder::default();
            let (_, stats) = evaluator.evaluate(&graph(files), &reg, &mut gui).unwrap();
            (stats.evaluated, gui.0)
        };

        assert_eq!(run(&files).0, 5);
        // Nothing changed: only the GUI sink runs, and it still shows both layers.
        assert_eq!(run(&files), (1, vec![("s".into(), 2), ("s".into(), 3)]));
        // Changing `a` re-runs a, wa and the sink; b and wb are reused.
        files[0].1 = "type = \"source\"\n[params]\nsize = 4";
        assert_eq!(run(&files), (3, vec![("s".into(), 4), ("s".into(), 3)]));
        // Rewiring wb to `a` re-runs wb (its source changed) and the sink.
        files[3].1 = "type = \"wrap\"\n[inputs]\nimage = \"a\"";
        assert_eq!(run(&files), (2, vec![("s".into(), 4), ("s".into(), 4)]));
    }
}
