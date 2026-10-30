//! Minimal node kinds used by core's own tests.

use std::sync::Arc;

use serde::Deserialize;

use crate::{
    EvalCtx, Inputs, Node, NodeResult, NodeSpec, Outputs, PortSpec, Registry, RgbaImage, Value,
    ValueType, parse_params,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceParams {
    #[serde(default = "one")]
    size: u32,
}

fn one() -> u32 {
    1
}

/// No inputs; outputs a `size`×`size` image.
struct Source;
impl Node for Source {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "source",
            inputs: vec![],
            outputs: vec![PortSpec::new("out", ValueType::Image)],
            gui: false,
        }
    }
    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        parse_params::<SourceParams>(params).map(|_| ())
    }
    fn eval(&self, _: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let p: SourceParams = parse_params(params)?;
        Ok(Outputs::out(Value::Image(Arc::new(RgbaImage::new(
            p.size, p.size,
        )))))
    }
}

/// Image → layer.
struct Wrap;
impl Node for Wrap {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "wrap",
            inputs: vec![PortSpec::new("image", ValueType::Image)],
            outputs: vec![PortSpec::new("out", ValueType::Layer)],
            gui: false,
        }
    }
    fn eval(&self, inputs: &Inputs, _: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let image = inputs.image("image")?.clone();
        Ok(Outputs::out(Value::Layer(crate::Layer {
            image,
            x: 0,
            y: 0,
        })))
    }
}

/// Image → image.
struct Pass;
impl Node for Pass {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "pass",
            inputs: vec![PortSpec::new("image", ValueType::Image)],
            outputs: vec![PortSpec::new("out", ValueType::Image)],
            gui: false,
        }
    }
    fn eval(&self, inputs: &Inputs, _: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        Ok(Outputs::out(Value::Image(inputs.image("image")?.clone())))
    }
}

/// GUI sink taking a list of layers; shows each one.
struct Sink;
impl Node for Sink {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "sink",
            inputs: vec![PortSpec::new("items", ValueType::Layer).multi()],
            outputs: vec![],
            gui: true,
        }
    }
    fn eval(&self, inputs: &Inputs, _: &toml::Table, ctx: &mut EvalCtx) -> NodeResult<Outputs> {
        for layer in inputs.layers("items")? {
            ctx.gui.show(ctx.node_id, layer.image.clone());
        }
        Ok(Outputs::none())
    }
}

pub fn registry() -> Registry {
    let mut r = Registry::new();
    r.register(Source);
    r.register(Wrap);
    r.register(Pass);
    r.register(Sink);
    r
}
