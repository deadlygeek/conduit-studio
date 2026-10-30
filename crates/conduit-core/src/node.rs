use std::collections::HashMap;
use std::fmt;
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

/// What kind of value a param holds. Used by editors and templates; nodes still validate
/// their params themselves (see [`Node::check_params`]).
#[derive(Debug, Clone, PartialEq)]
pub enum ParamKind {
    Int { min: i64, max: i64 },
    Float { min: f64, max: f64 },
    Color,
    Text,
}

impl fmt::Display for ParamKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParamKind::Int { min, max } if *min == i64::MIN && *max == i64::MAX => {
                f.write_str("int")
            }
            ParamKind::Int { min, max } => write!(f, "int {min}..{max}"),
            ParamKind::Float { min, max } if min.is_infinite() && max.is_infinite() => {
                f.write_str("float")
            }
            ParamKind::Float { min, max } => write!(f, "float {min}..{max}"),
            ParamKind::Color => f.write_str("color"),
            ParamKind::Text => f.write_str("text"),
        }
    }
}

/// Describes one `[params]` entry of a node kind.
#[derive(Debug, Clone)]
pub struct ParamSpec {
    pub name: &'static str,
    pub kind: ParamKind,
    /// Must be present in the node file.
    pub required: bool,
    /// Value used when absent, as a TOML literal (optional params only).
    pub default: Option<&'static str>,
    /// Value written into new nodes, as a TOML literal (required params).
    pub example: &'static str,
    pub doc: &'static str,
}

impl ParamSpec {
    fn new(name: &'static str, kind: ParamKind, example: &'static str) -> Self {
        Self {
            name,
            kind,
            required: true,
            default: None,
            example,
            doc: "",
        }
    }

    pub fn int(name: &'static str, min: i64, max: i64, example: &'static str) -> Self {
        Self::new(name, ParamKind::Int { min, max }, example)
    }

    pub fn float(name: &'static str, min: f64, max: f64, example: &'static str) -> Self {
        Self::new(name, ParamKind::Float { min, max }, example)
    }

    pub fn color(name: &'static str, example: &'static str) -> Self {
        Self::new(name, ParamKind::Color, example)
    }

    pub fn text(name: &'static str, example: &'static str) -> Self {
        Self::new(name, ParamKind::Text, example)
    }

    /// Makes the param optional with the given default (a TOML literal).
    pub fn default(mut self, default: &'static str) -> Self {
        self.required = false;
        self.default = Some(default);
        self
    }

    /// Makes the param optional with no default (e.g. "use the node id").
    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    pub fn doc(mut self, doc: &'static str) -> Self {
        self.doc = doc;
        self
    }

    /// Parses user input (as typed in an editor) into a TOML value of this param's kind.
    /// Text and colors are taken literally, without quotes.
    pub fn parse_input(&self, input: &str) -> Result<toml::Value, String> {
        let input = input.trim();
        match &self.kind {
            ParamKind::Int { min, max } => {
                let v: i64 = input
                    .parse()
                    .map_err(|_| format!("`{input}` is not a whole number"))?;
                if !(*min..=*max).contains(&v) {
                    return Err(format!("must be between {min} and {max}"));
                }
                Ok(toml::Value::Integer(v))
            }
            ParamKind::Float { min, max } => {
                let v: f64 = input
                    .parse()
                    .map_err(|_| format!("`{input}` is not a number"))?;
                if !(v >= *min && v <= *max) {
                    return Err(format!("must be between {min} and {max}"));
                }
                Ok(toml::Value::Float(v))
            }
            ParamKind::Color => {
                crate::color::parse_color(input).map(|_| toml::Value::String(input.to_string()))
            }
            ParamKind::Text => Ok(toml::Value::String(input.to_string())),
        }
    }
}

/// Static description of a node kind.
#[derive(Debug, Clone)]
pub struct NodeSpec {
    pub kind: &'static str,
    pub inputs: Vec<PortSpec>,
    pub outputs: Vec<PortSpec>,
    pub params: Vec<ParamSpec>,
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

    pub fn param(&self, name: &str) -> Option<&ParamSpec> {
        self.params.iter().find(|p| p.name == name)
    }

    /// Contents for a new node file of this kind: required params get their example value,
    /// optional ones and inputs are listed as comments.
    pub fn template(&self) -> String {
        let mut out = format!("type = \"{}\"\n", self.kind);
        if !self.inputs.is_empty() {
            out.push_str("\n[inputs]\n");
            for p in &self.inputs {
                let value = if p.multi { "[\"node\"]" } else { "\"node\"" };
                out.push_str(&format!("# {} = {value}  # {}\n", p.name, p.ty));
            }
        }
        if !self.params.is_empty() {
            out.push_str("\n[params]\n");
            for p in &self.params {
                if p.required {
                    out.push_str(&format!("{} = {}\n", p.name, p.example));
                } else {
                    let value = p.default.unwrap_or(p.example);
                    out.push_str(&format!("# {} = {value}\n", p.name));
                }
            }
        }
        out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_input_by_kind() {
        let int = ParamSpec::int("w", 1, 10, "5");
        assert_eq!(int.parse_input(" 7 "), Ok(toml::Value::Integer(7)));
        assert!(int.parse_input("0").is_err());
        assert!(int.parse_input("1.5").is_err());

        let float = ParamSpec::float("a", 0.0, 1.0, "1.0");
        assert_eq!(float.parse_input("0.25"), Ok(toml::Value::Float(0.25)));
        assert!(float.parse_input("2").is_err());
        assert!(float.parse_input("NaN").is_err());

        let color = ParamSpec::color("c", "\"white\"");
        assert_eq!(
            color.parse_input("#ff0000"),
            Ok(toml::Value::String("#ff0000".into()))
        );
        assert!(color.parse_input("nope").is_err());

        let text = ParamSpec::text("t", "\"x\"");
        assert_eq!(
            text.parse_input("hello world"),
            Ok(toml::Value::String("hello world".into()))
        );
    }

    #[test]
    fn template_lists_ports_and_params() {
        let spec = NodeSpec {
            kind: "demo",
            inputs: vec![
                PortSpec::new("image", ValueType::Image),
                PortSpec::new("layers", ValueType::Layer).multi(),
            ],
            outputs: vec![PortSpec::new("out", ValueType::Image)],
            params: vec![
                ParamSpec::int("size", 1, 9, "3"),
                ParamSpec::int("x", i64::MIN, i64::MAX, "0").default("0"),
            ],
            gui: false,
        };
        let text = spec.template();
        assert_eq!(
            text,
            "type = \"demo\"\n\n[inputs]\n# image = \"node\"  # image\n# layers = [\"node\"]  # layer\n\n[params]\nsize = 3\n# x = 0\n"
        );
        let parsed: toml::Table = toml::from_str(&text).unwrap();
        assert_eq!(parsed["params"]["size"].as_integer(), Some(3));
    }

    #[test]
    fn param_kind_display() {
        assert_eq!(
            ParamKind::Int {
                min: i64::MIN,
                max: i64::MAX
            }
            .to_string(),
            "int"
        );
        assert_eq!(ParamKind::Int { min: 1, max: 5 }.to_string(), "int 1..5");
        assert_eq!(ParamKind::Color.to_string(), "color");
    }
}
