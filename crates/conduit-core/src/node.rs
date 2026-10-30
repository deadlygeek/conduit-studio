use std::collections::HashMap;
use std::sync::Arc;

use image::RgbaImage;
use serde::de::DeserializeOwned;

use crate::value::{Layer, Value, ValueType};

/// Describes one input or output port of a node kind.
#[derive(Debug, Clone)]
pub struct PortSpec {
    pub name: &'static str,
    pub ty: ValueType,
    /// Accepts a list of connections (written as a TOML array). Inputs only.
    pub multi: bool,
    /// May be left unconnected. Inputs only.
    pub optional: bool,
}

impl PortSpec {
    pub const fn new(name: &'static str, ty: ValueType) -> Self {
        Self {
            name,
            ty,
            multi: false,
            optional: false,
        }
    }

    pub const fn multi(mut self) -> Self {
        self.multi = true;
        self
    }

    pub const fn optional(mut self) -> Self {
        self.optional = true;
        self
    }
}

/// Static description of a node kind.
#[derive(Debug, Clone)]
pub struct NodeSpec {
    pub kind: &'static str,
    pub inputs: Vec<PortSpec>,
    pub outputs: Vec<PortSpec>,
    /// GUI nodes are backed by a separate process launched by the runner.
    pub gui: bool,
}

impl NodeSpec {
    pub fn input(&self, name: &str) -> Option<&PortSpec> {
        self.inputs.iter().find(|p| p.name == name)
    }

    pub fn output(&self, name: &str) -> Option<&PortSpec> {
        self.outputs.iter().find(|p| p.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct NodeError(pub String);

impl NodeError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

pub type NodeResult<T> = Result<T, NodeError>;

/// A node kind. Nodes know only their own ports and params, never other nodes.
pub trait Node: Send + Sync {
    fn spec(&self) -> NodeSpec;

    /// Validates params at load time so mistakes are reported before evaluation.
    fn check_params(&self, _params: &toml::Table) -> NodeResult<()> {
        Ok(())
    }

    fn eval(&self, inputs: &Inputs, params: &toml::Table, ctx: &mut EvalCtx)
    -> NodeResult<Outputs>;
}

/// Deserializes a node's `[params]` table into its typed params struct.
pub fn parse_params<T: DeserializeOwned>(params: &toml::Table) -> NodeResult<T> {
    toml::Value::Table(params.clone())
        .try_into()
        .map_err(|e: toml::de::Error| NodeError::new(e.message().to_string()))
}

/// Input values for one evaluation, already coerced to each port's declared type.
#[derive(Debug, Default)]
pub struct Inputs {
    values: HashMap<String, Vec<Value>>,
}

impl Inputs {
    pub fn new(values: HashMap<String, Vec<Value>>) -> Self {
        Self { values }
    }

    /// All values connected to `port` (empty if unconnected).
    pub fn all(&self, port: &str) -> &[Value] {
        self.values.get(port).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn get(&self, port: &str) -> Option<&Value> {
        self.all(port).first()
    }

    pub fn image(&self, port: &str) -> NodeResult<&Arc<RgbaImage>> {
        match self.get(port) {
            Some(Value::Image(img)) => Ok(img),
            other => Err(type_error(port, "image", other)),
        }
    }

    pub fn layers(&self, port: &str) -> NodeResult<Vec<&Layer>> {
        self.all(port)
            .iter()
            .map(|v| match v {
                Value::Layer(l) => Ok(l),
                other => Err(type_error(port, "layer", Some(other))),
            })
            .collect()
    }
}

fn type_error(port: &str, expected: &str, found: Option<&Value>) -> NodeError {
    match found {
        Some(v) => NodeError::new(format!(
            "input `{port}`: expected {expected}, got {}",
            v.ty()
        )),
        None => NodeError::new(format!("input `{port}` is not connected")),
    }
}

/// Output values produced by a node, keyed by port name.
#[derive(Debug, Default)]
pub struct Outputs {
    pub values: HashMap<String, Value>,
}

impl Outputs {
    pub fn none() -> Self {
        Self::default()
    }

    /// The common case: a single output named `out`.
    pub fn out(value: Value) -> Self {
        Self {
            values: HashMap::from([("out".to_string(), value)]),
        }
    }
}

/// Receives images from GUI sink nodes (e.g. preview). The runner implements this by
/// forwarding frames to the node's GUI process; headless runs use [`NullHost`].
pub trait GuiHost {
    fn show(&mut self, node_id: &str, image: Arc<RgbaImage>);
}

pub struct NullHost;

impl GuiHost for NullHost {
    fn show(&mut self, _node_id: &str, _image: Arc<RgbaImage>) {}
}

pub struct EvalCtx<'a> {
    pub node_id: &'a str,
    pub gui: &'a mut dyn GuiHost,
}
