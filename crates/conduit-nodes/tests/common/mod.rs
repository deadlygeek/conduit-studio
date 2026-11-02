//! Helpers shared by the `example_*` tests.
#![allow(dead_code)] // each test crate uses a different subset

use std::path::PathBuf;

use conduit_core::{
    Graph, Layer, NullHost, Reference, Registry, RgbaImage, Value, Values, evaluate, load,
};

/// `examples/<name>` in the repository.
pub fn example_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name)
}

/// Loads `examples/<name>` with the standard registry.
pub fn load_example(name: &str) -> (PathBuf, Registry, Graph) {
    let dir = example_dir(name);
    let registry = conduit_nodes::registry();
    let graph = load(&dir, &registry).unwrap_or_else(|e| panic!("{e}"));
    (dir, registry, graph)
}

/// Evaluates `examples/<name>`.
pub fn render(name: &str) -> Values {
    let (_, registry, graph) = load_example(name);
    evaluate(&graph, &registry, &mut NullHost).unwrap()
}

/// The image output `id` (e.g. `"comp"`).
pub fn image<'a>(values: &'a Values, id: &str) -> &'a RgbaImage {
    match &values[&Reference::parse(id).unwrap()] {
        Value::Image(img) => img,
        other => panic!("{id}: expected image, got {other:?}"),
    }
}

/// The layer output `id`.
pub fn layer(values: &Values, id: &str) -> Layer {
    match &values[&Reference::parse(id).unwrap()] {
        Value::Layer(l) => l.clone(),
        other => panic!("{id}: expected layer, got {other:?}"),
    }
}
