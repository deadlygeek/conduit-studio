//! Evaluating a loaded graph.

use std::collections::HashMap;

use crate::graph::{Graph, Reference};
use crate::node::{EvalCtx, GuiHost, Inputs};
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

/// Evaluates every node in order. The graph must have been loaded with the same registry.
pub fn evaluate(
    graph: &Graph,
    registry: &Registry,
    gui: &mut dyn GuiHost,
) -> Result<Values, EvalError> {
    let mut values = Values::new();
    for node in &graph.nodes {
        let node_impl = registry
            .get(&node.kind)
            .expect("graph loaded with this registry");
        let spec = node_impl.spec();

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

        let mut ctx = EvalCtx {
            node_id: &node.id,
            gui: &mut *gui,
        };
        let outputs = node_impl
            .eval(&Inputs::new(inputs), &node.params, &mut ctx)
            .map_err(|e| EvalError::Node {
                node: node.id.clone(),
                message: e.0,
            })?;

        let mut outputs = outputs.values;
        for out in &spec.outputs {
            match outputs.remove(out.name) {
                Some(v) if v.ty() == out.ty => {
                    values.insert(
                        Reference {
                            node: node.id.clone(),
                            port: out.name.to_string(),
                        },
                        v,
                    );
                }
                _ => {
                    return Err(EvalError::BadOutput {
                        node: node.id.clone(),
                        port: out.name.to_string(),
                        expected: out.ty.to_string(),
                    });
                }
            }
        }
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::*;
    use crate::RgbaImage;
    use crate::graph::from_sources;
    use crate::test_nodes::registry;

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
        let graph = from_sources(
            files
                .iter()
                .map(|(id, t)| (PathBuf::from(format!("{id}.toml")), t.to_string())),
            &reg,
        )
        .unwrap();

        let mut gui = Recorder::default();
        let values = evaluate(&graph, &reg, &mut gui).unwrap();

        let w_out = Reference::parse("w").unwrap();
        assert!(matches!(&values[&w_out], Value::Layer(l) if l.image.width() == 7));
        assert_eq!(gui.0, [("s".to_string(), 7), ("s".to_string(), 7)]);
    }
}
