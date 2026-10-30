use std::collections::HashMap;
use std::sync::Arc;

use conduit_core::{EvalCtx, Inputs, Node, NodeResult, NullHost, Rgba, RgbaImage, Value};

pub fn params(toml_src: &str) -> toml::Table {
    toml::from_str(toml_src).expect("valid test toml")
}

pub fn image(w: u32, h: u32, rgba: [u8; 4]) -> Arc<RgbaImage> {
    Arc::new(RgbaImage::from_pixel(w, h, Rgba(rgba)))
}

/// Evaluates a node directly and returns its `out` value.
pub fn run(
    node: &dyn Node,
    inputs: Vec<(&str, Vec<Value>)>,
    params: &toml::Table,
) -> NodeResult<Value> {
    let inputs = inputs
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect::<HashMap<_, _>>();
    let mut ctx = EvalCtx {
        node_id: "test",
        gui: &mut NullHost,
    };
    let mut outputs = node.eval(&Inputs::new(inputs), params, &mut ctx)?;
    Ok(outputs.values.remove("out").expect("node produced `out`"))
}
