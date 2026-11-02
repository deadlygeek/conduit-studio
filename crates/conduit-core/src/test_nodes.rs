//! Minimal node kinds used by core's own tests.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;

use crate::{
    EvalCtx, Inputs, Node, NodeResult, NodeSpec, Outputs, ParamSpec, PortSpec, Registry, RgbaImage,
    Value, ValueType, parse_params,
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
            params: vec![ParamSpec::int("size", 0, 64, "1").default("1")],
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
            params: vec![],
            gui: false,
        }
    }
    fn eval(&self, inputs: &Inputs, _: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let image = inputs.image("image")?.clone();
        Ok(Outputs::out(Value::Layer(crate::Layer::new(image))))
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
            params: vec![],
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
            params: vec![],
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NumParams {
    #[serde(default)]
    value: f64,
}

/// Outputs its `value` param as a number.
struct Num;
impl Node for Num {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "num",
            inputs: vec![],
            outputs: vec![PortSpec::new("out", ValueType::Number)],
            params: vec![ParamSpec::float("value", -1e9, 1e9, "0.0").default("0.0")],
            gui: false,
        }
    }
    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        parse_params::<NumParams>(params).map(|_| ())
    }
    fn eval(&self, _: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let p: NumParams = parse_params(params)?;
        Ok(Outputs::out(Value::Number(p.value)))
    }
}

/// Each entry of its `colors` table is a color output.
struct Pal;
impl Node for Pal {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "pal",
            inputs: vec![],
            outputs: vec![PortSpec::new("colors", ValueType::Color).entries()],
            params: vec![ParamSpec::colors("colors", "{ a = \"red\" }")],
            gui: false,
        }
    }
    fn eval(&self, _: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let colors = params.get("colors").and_then(|c| c.as_table());
        let values = colors
            .into_iter()
            .flatten()
            .map(|(k, v)| {
                let c = crate::parse_color(v.as_str().unwrap_or_default())
                    .unwrap_or(image::Rgba([0; 4]));
                (k.clone(), Value::Color(c))
            })
            .collect();
        Ok(Outputs { values })
    }
}

/// Takes a color (as a param that can be linked) and outputs it.
struct Tint;
impl Node for Tint {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "tint",
            inputs: vec![],
            outputs: vec![PortSpec::new("out", ValueType::Color)],
            params: vec![ParamSpec::color("color", "\"white\"")],
            gui: false,
        }
    }
    fn eval(&self, _: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let c = params
            .get("color")
            .and_then(|c| c.as_str())
            .unwrap_or("white");
        Ok(Outputs::out(Value::Color(
            crate::parse_color(c).map_err(crate::NodeError::new)?,
        )))
    }
}

pub fn registry() -> Registry {
    let mut r = Registry::new();
    r.register(Source);
    r.register(Wrap);
    r.register(Pass);
    r.register(Sink);
    r.register(Num);
    r.register(Pal);
    r.register(Tint);
    r
}

/// `(id, text)` pairs as node file sources.
pub fn sources(files: &[(&str, &str)]) -> crate::graph::Sources {
    files
        .iter()
        .map(|(id, text)| (PathBuf::from(format!("{id}.toml")), text.to_string()))
        .collect()
}
