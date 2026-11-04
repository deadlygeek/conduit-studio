use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
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
    /// Feeds the number, color or font param of the same name, overriding its value (see
    /// [`NodeSpec::with_param_inputs`]). Inputs only.
    pub param: bool,
    /// Outputs only: stands for one output per entry of the table param with the same
    /// name (e.g. a palette's `colors`), named after the entry's key.
    pub entries: bool,
    /// Outputs only: stands for outputs `{name}1`..`{name}N`, where N is the product of
    /// the named params, each fixed (see [`ParamSpec::fixed`]): an int param counts as its
    /// value, a text param as its number of non-whitespace characters. Empty for an
    /// ordinary output.
    pub repeat: &'static [&'static str],
}

impl PortSpec {
    pub const fn new(name: &'static str, ty: ValueType) -> Self {
        Self {
            name,
            ty,
            multi: false,
            optional: false,
            param: false,
            entries: false,
            repeat: &[],
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

    /// See [`PortSpec::entries`].
    pub const fn entries(mut self) -> Self {
        self.entries = true;
        self
    }

    /// See [`PortSpec::repeat`].
    pub const fn repeated(mut self, params: &'static [&'static str]) -> Self {
        self.repeat = params;
        self
    }
}

/// What kind of value a param holds. Used by editors and templates, and for defaults and
/// range checks in [`NodeSpec::parse_params`].
#[derive(Debug, Clone, PartialEq)]
pub enum ParamKind {
    Int {
        min: i64,
        max: i64,
    },
    Float {
        min: f64,
        max: f64,
    },
    Color,
    Text,
    /// A text param naming an installed font family; editors offer a font picker.
    Font,
    /// A text param holding a path relative to the project folder; editors offer the
    /// project's files with these extensions (lowercase, without the dot).
    File {
        extensions: &'static [&'static str],
    },
    /// A table of named colors (`[params.colors]`), each also an output.
    Colors,
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
            ParamKind::Font => f.write_str("font"),
            ParamKind::File { .. } => f.write_str("file"),
            ParamKind::Colors => f.write_str("named colors"),
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
    /// Gets an input so another node can feed it (number, color and font params only).
    /// False for params that decide the node's outputs.
    pub linkable: bool,
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
            linkable: true,
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

    /// A table of named colors; `example` is an inline table literal.
    pub fn colors(name: &'static str, example: &'static str) -> Self {
        Self::new(name, ParamKind::Colors, example)
    }

    pub fn text(name: &'static str, example: &'static str) -> Self {
        Self::new(name, ParamKind::Text, example)
    }

    /// A font family name (text, with a font picker in editors).
    pub fn font(name: &'static str, example: &'static str) -> Self {
        Self::new(name, ParamKind::Font, example)
    }

    /// A path relative to the project folder (text, with a file picker in editors).
    pub fn file(
        name: &'static str,
        extensions: &'static [&'static str],
        example: &'static str,
    ) -> Self {
        Self::new(name, ParamKind::File { extensions }, example)
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

    /// Typed into the node file only: no input feeds it (see [`ParamSpec::linkable`]).
    pub fn fixed(mut self) -> Self {
        self.linkable = false;
        self
    }

    pub fn doc(mut self, doc: &'static str) -> Self {
        self.doc = doc;
        self
    }

    /// The default as a value, if the param has one.
    pub fn default_value(&self) -> Option<toml::Value> {
        self.default.and_then(parse_literal)
    }

    /// Checks a number param's value against its range. Other params, and values of the
    /// wrong type (left for deserializing to report), pass.
    pub fn check_range(&self, value: &toml::Value) -> Result<(), String> {
        let out_of_range = |min: &dyn fmt::Display, max: &dyn fmt::Display| {
            Err(format!(
                "`{}` must be between {min} and {max}, got {value}",
                self.name
            ))
        };
        match (&self.kind, value) {
            (ParamKind::Int { min, max }, toml::Value::Integer(v)) if !(min..=max).contains(&v) => {
                out_of_range(min, max)
            }
            (ParamKind::Float { min, max }, toml::Value::Integer(_) | toml::Value::Float(_)) => {
                let v = value
                    .as_float()
                    .unwrap_or_else(|| value.as_integer().unwrap() as f64);
                // Written so NaN fails too.
                if v >= *min && v <= *max {
                    Ok(())
                } else {
                    out_of_range(min, max)
                }
            }
            _ => Ok(()),
        }
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
                Ok(float_value(v))
            }
            ParamKind::Color => {
                crate::color::parse_color(input).map(|_| toml::Value::String(input.to_string()))
            }
            ParamKind::Text | ParamKind::Font | ParamKind::File { .. } => {
                Ok(toml::Value::String(input.to_string()))
            }
            ParamKind::Colors => Err("edit the colors one at a time".into()),
        }
    }
}

/// Parses a single TOML literal (`5`, `"text"`, `{ a = 1 }`).
pub fn parse_literal(literal: &str) -> Option<toml::Value> {
    let mut table: toml::Table = toml::from_str(&format!("v = {literal}")).ok()?;
    table.remove("v")
}

/// A float param's value as written to a node file: whole numbers without the `.0`
/// (`x = 120`, not `x = 120.0`). Float params accept either.
pub fn float_value(v: f64) -> toml::Value {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        toml::Value::Integer(v as i64)
    } else {
        toml::Value::Float(v)
    }
}

/// The most outputs a repeated output (see [`PortSpec::repeat`]) can stand for.
pub const MAX_REPEAT: usize = 256;

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

    /// Every output of a node with these params, in order: entry outputs (see
    /// [`PortSpec::entries`]) expand to one per table key; keys that aren't valid port
    /// names are skipped (load reports them).
    pub fn outputs_for(&self, params: &toml::Table) -> Vec<(String, ValueType)> {
        let mut out = Vec::new();
        for port in &self.outputs {
            if !port.repeat.is_empty() {
                for i in 1..=self.repeat_count(port, params) {
                    out.push((format!("{}{i}", port.name), port.ty));
                }
                continue;
            }
            if !port.entries {
                out.push((port.name.to_string(), port.ty));
                continue;
            }
            let keys = params.get(port.name).and_then(|v| v.as_table());
            for key in keys.into_iter().flat_map(|t| t.keys()) {
                if crate::graph::is_ident(key) {
                    out.push((key.clone(), port.ty));
                }
            }
        }
        out
    }

    /// How many outputs a repeated output (see [`PortSpec::repeat`]) stands for with these
    /// params. Out-of-range ints are clamped to 0..=max here (load reports them), and
    /// the total is capped at [`MAX_REPEAT`].
    pub fn repeat_count(&self, port: &PortSpec, params: &toml::Table) -> usize {
        let mut n: usize = 1;
        for name in port.repeat {
            let Some(spec) = self.param(name) else {
                return 0;
            };
            let value = params.get(*name).cloned().or_else(|| spec.default_value());
            let count = match (&spec.kind, value) {
                (ParamKind::Int { max, .. }, Some(toml::Value::Integer(v))) => {
                    v.clamp(0, (*max).max(0)) as usize
                }
                (ParamKind::Text, Some(toml::Value::String(t))) => {
                    t.chars().filter(|c| !c.is_whitespace()).count()
                }
                _ => 0,
            };
            n = n.saturating_mul(count);
        }
        n.min(MAX_REPEAT)
    }

    /// The type of output `port` on a node with these params.
    pub fn output_type(&self, params: &toml::Table, port: &str) -> Option<ValueType> {
        self.outputs_for(params)
            .into_iter()
            .find(|(name, _)| name == port)
            .map(|(_, ty)| ty)
    }

    pub fn param(&self, name: &str) -> Option<&ParamSpec> {
        self.params.iter().find(|p| p.name == name)
    }

    /// Adds an optional input for every number, color and font param (a `number`, `color`
    /// or `font` input of the same name), so the value can be typed into the node file or
    /// fed from another node's output. Number and color inputs are written over the param
    /// before `eval`; a font input is left for the node to read from its inputs. The registry applies this to every kind. Number params must
    /// have a default.
    ///
    /// Panics if a number param has no default or a param's name clashes with an input.
    pub fn with_param_inputs(mut self) -> Self {
        for param in self.outputs.iter().flat_map(|o| o.repeat) {
            let p = self.param(param);
            assert!(
                p.is_some_and(|p| !p.linkable
                    && match p.kind {
                        ParamKind::Int { .. } => p.default.is_some(),
                        ParamKind::Text => true,
                        _ => false,
                    }),
                "`{}`: repeat param `{param}` must be a fixed int param with a default, \
                 or a fixed text param",
                self.kind
            );
        }
        for p in self.params.iter().filter(|p| p.linkable) {
            let ty = match p.kind {
                ParamKind::Int { .. } | ParamKind::Float { .. } => ValueType::Number,
                ParamKind::Color => ValueType::Color,
                ParamKind::Font => ValueType::Font,
                ParamKind::Text | ParamKind::File { .. } | ParamKind::Colors => continue,
            };
            assert!(
                ty != ValueType::Number || p.default.is_some(),
                "`{}`: number param `{}` needs a default",
                self.kind,
                p.name
            );
            assert!(
                self.input(p.name).is_none(),
                "`{}`: param `{}` clashes with an input",
                self.kind,
                p.name
            );
            self.inputs.push(PortSpec {
                param: true,
                ..PortSpec::new(p.name, ty).optional()
            });
        }
        self
    }

    /// `params` as validation should see them when the params in `linked` are fed from
    /// inputs: each linked param missing from the file gets its example value, so a
    /// required param (e.g. `rect.color`) may be linked instead of typed.
    pub fn params_with_links<'a>(
        &self,
        params: &toml::Table,
        linked: impl IntoIterator<Item = &'a str>,
    ) -> toml::Table {
        let mut params = params.clone();
        for name in linked {
            let Some(p) = self.param(name) else { continue };
            if self.input(name).is_some_and(|i| i.param)
                && !params.contains_key(name)
                && let Some(v) = parse_literal(p.example)
            {
                params.insert(name.to_string(), v);
            }
        }
        params
    }

    /// Deserializes `params` into a node's typed params struct, after filling in every
    /// missing param that has a default and checking number params against their ranges.
    pub fn parse_params<T: DeserializeOwned>(&self, params: &toml::Table) -> NodeResult<T> {
        let mut params = params.clone();
        for p in &self.params {
            match params.get(p.name) {
                Some(v) => p.check_range(v).map_err(NodeError)?,
                None => {
                    if let Some(v) = p.default_value() {
                        params.insert(p.name.to_string(), v);
                    }
                }
            }
        }
        parse_params(&params)
    }

    /// Contents for a new node file of this kind: required params get their example value,
    /// optional ones and inputs are listed as comments.
    pub fn template(&self) -> String {
        let mut out = format!("type = \"{}\"\n", self.kind);
        if self.inputs.iter().any(|p| !p.param) {
            out.push_str("\n[inputs]\n");
            for p in self.inputs.iter().filter(|p| !p.param) {
                let value = if p.multi { "[\"node\"]" } else { "\"node\"" };
                out.push_str(&format!("# {} = {value}  # {}\n", p.name, p.ty));
            }
        }
        let (tables, params): (Vec<&ParamSpec>, Vec<&ParamSpec>) = self
            .params
            .iter()
            .partition(|p| p.kind == ParamKind::Colors);
        if !params.is_empty() {
            out.push_str("\n[params]\n");
            for p in params {
                if p.required {
                    out.push_str(&format!("{} = {}\n", p.name, p.example));
                } else {
                    let value = p.default.unwrap_or(p.example);
                    out.push_str(&format!("# {} = {value}\n", p.name));
                }
            }
        }
        // Tables get a section each: `[params.colors]` with one line per entry.
        for p in tables {
            out.push_str(&format!("\n[params.{}]\n", p.name));
            let entries = parse_literal(p.example);
            for (k, v) in entries.iter().filter_map(|v| v.as_table()).flatten() {
                out.push_str(&format!("{k} = {v}\n"));
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

    /// Files this node reads (relative to the project folder), besides its node file. When
    /// one changes, the node re-runs and `conduit run` re-renders.
    fn files(&self, _params: &toml::Table) -> Vec<PathBuf> {
        Vec::new()
    }

    /// For nodes that generate several outputs (`array`, `split_text`): the ordinary nodes
    /// that would produce the same outputs, so an editor can replace this node with them
    /// for good ("detach"). Takes what [`Node::eval`] takes. `None` if the kind can't be
    /// detached.
    fn detach(
        &self,
        _inputs: &Inputs,
        _params: &toml::Table,
        _ctx: &mut EvalCtx,
    ) -> Option<NodeResult<DetachPlan>> {
        None
    }
}

/// What a node becomes when detached (see [`Node::detach`]).
#[derive(Debug, Clone, PartialEq)]
pub struct DetachPlan {
    pub nodes: Vec<DetachedNode>,
    /// For each output of the detached node, the new node (by [`DetachedNode::name`])
    /// whose `out` replaces it.
    pub outputs: Vec<(String, String)>,
}

/// One node of a [`DetachPlan`].
#[derive(Debug, Clone, PartialEq)]
pub struct DetachedNode {
    /// Appended to the detached node's id: `grid` + `out6` → `grid_out6`.
    pub name: String,
    /// The first line of the new file, as a comment.
    pub comment: String,
    pub kind: &'static str,
    pub inputs: Vec<(String, DetachedSource)>,
    pub params: toml::Table,
}

/// Where an input of a [`DetachedNode`] comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetachedSource {
    /// Whatever fed this input port of the detached node (left out if nothing did).
    Input(String),
    /// Another node of the same plan, by [`DetachedNode::name`].
    Node(String),
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

    pub fn layer(&self, port: &str) -> NodeResult<&Layer> {
        match self.get(port) {
            Some(Value::Layer(l)) => Ok(l),
            other => Err(type_error(port, "layer", other)),
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
    /// The project folder (see [`crate::Graph::root`]).
    pub root: &'a Path,
    pub gui: &'a mut dyn GuiHost,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_params_fills_defaults_and_checks_ranges() {
        #[derive(serde::Deserialize, Debug, PartialEq)]
        struct P {
            n: i64,
            f: f64,
            c: String,
        }
        let spec = NodeSpec {
            kind: "k",
            inputs: vec![],
            outputs: vec![],
            params: vec![
                ParamSpec::int("n", 0, 10, "1").default("3"),
                ParamSpec::float("f", -1.0, 1.0, "0").default("0.5"),
                ParamSpec::color("c", "\"red\"").default("\"white\""),
            ],
            gui: false,
        };
        let parse = |s: &str| spec.parse_params::<P>(&toml::from_str(s).unwrap());
        assert_eq!(
            parse("n = 7"),
            Ok(P {
                n: 7,
                f: 0.5,
                c: "white".into()
            })
        );
        assert_eq!(parse("f = 1").map(|p| p.f), Ok(1.0));
        assert_eq!(
            parse("n = 11").unwrap_err().0,
            "`n` must be between 0 and 10, got 11"
        );
        assert_eq!(
            parse("f = -1.5").unwrap_err().0,
            "`f` must be between -1 and 1, got -1.5"
        );
        assert!(parse("f = nan").is_err());
        // Wrong types are left for serde to report.
        assert!(parse("n = \"x\"").unwrap_err().0.contains("invalid type"));
    }

    #[test]
    fn parse_input_by_kind() {
        let int = ParamSpec::int("w", 1, 10, "5");
        assert_eq!(int.parse_input(" 7 "), Ok(toml::Value::Integer(7)));
        assert!(int.parse_input("0").is_err());
        assert!(int.parse_input("1.5").is_err());

        let float = ParamSpec::float("a", 0.0, 1.0, "1.0");
        assert_eq!(float.parse_input("0.25"), Ok(toml::Value::Float(0.25)));
        assert_eq!(float.parse_input("1"), Ok(toml::Value::Integer(1)));
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

    #[test]
    fn number_params_get_inputs() {
        let spec = NodeSpec {
            kind: "demo",
            inputs: vec![PortSpec::new("image", ValueType::Image)],
            outputs: vec![],
            params: vec![
                ParamSpec::int("x", 0, 9, "0").default("0"),
                ParamSpec::float("a", 0.0, 1.0, "1.0").default("1.0"),
                ParamSpec::color("c", "\"white\""),
            ],
            gui: false,
        }
        .with_param_inputs();
        let names: Vec<_> = spec.inputs.iter().map(|p| (p.name, p.param)).collect();
        assert_eq!(
            names,
            [("image", false), ("x", true), ("a", true), ("c", true)]
        );
        assert_eq!(spec.input("c").unwrap().ty, ValueType::Color);
        let x = spec.input("x").unwrap();
        assert!(x.optional && !x.multi && x.ty == ValueType::Number);
        assert!(
            !spec.template().contains("# x = \"node\""),
            "{}",
            spec.template()
        );
    }

    fn repeat_spec(count: ParamSpec) -> NodeSpec {
        NodeSpec {
            kind: "dup",
            inputs: vec![],
            outputs: vec![PortSpec::new("out", ValueType::Layer).repeated(&["count"])],
            params: vec![count],
            gui: false,
        }
    }

    #[test]
    fn repeated_outputs_follow_their_param() {
        let spec = repeat_spec(ParamSpec::int("count", 1, 4, "2").default("2").fixed())
            .with_param_inputs();
        let names = |s: &str| -> Vec<String> {
            spec.outputs_for(&toml::from_str(s).unwrap())
                .into_iter()
                .map(|(n, _)| n)
                .collect()
        };
        assert_eq!(names(""), ["out1", "out2"]);
        assert_eq!(names("count = 3"), ["out1", "out2", "out3"]);
        assert_eq!(names("count = 99").len(), 4);
        assert_eq!(names("count = -1"), Vec::<String>::new());
        assert_eq!(
            spec.output_type(&toml::Table::new(), "out2"),
            Some(ValueType::Layer)
        );
        // A fixed param gets no input.
        assert!(spec.input("count").is_none());
    }

    #[test]
    fn repeated_outputs_multiply_their_params() {
        let spec = NodeSpec {
            kind: "grid",
            inputs: vec![],
            outputs: vec![PortSpec::new("out", ValueType::Layer).repeated(&["cols", "rows"])],
            params: vec![
                ParamSpec::int("cols", 1, 10, "3").default("3").fixed(),
                ParamSpec::int("rows", 1, 10, "1").default("1").fixed(),
            ],
            gui: false,
        }
        .with_param_inputs();
        let count = |s: &str| spec.outputs_for(&toml::from_str(s).unwrap()).len();
        assert_eq!(count(""), 3);
        assert_eq!(count("rows = 2"), 6);
        assert_eq!(count("cols = 10\nrows = 10"), 100);
    }

    #[test]
    fn repeated_outputs_count_text_characters() {
        let spec = NodeSpec {
            kind: "split",
            inputs: vec![],
            outputs: vec![PortSpec::new("char", ValueType::Layer).repeated(&["text"])],
            params: vec![ParamSpec::text("text", "\"Hi\"").fixed()],
            gui: false,
        }
        .with_param_inputs();
        let count = |s: &str| spec.outputs_for(&toml::from_str(s).unwrap()).len();
        assert_eq!(count(""), 0, "missing text: no outputs");
        assert_eq!(count("text = \"HELLO\""), 5);
        assert_eq!(count("text = \"a b\\nc\""), 3, "whitespace is skipped");
        assert_eq!(count("text = \"é✓\""), 2, "characters, not bytes");
        let long = format!("text = \"{}\"", "x".repeat(1000));
        assert_eq!(count(&long), MAX_REPEAT);
    }

    #[test]
    #[should_panic(expected = "must be a fixed int param")]
    fn repeat_params_must_be_fixed() {
        let _ = repeat_spec(ParamSpec::int("count", 1, 4, "2").default("2")).with_param_inputs();
    }

    #[test]
    #[should_panic(expected = "needs a default")]
    fn number_params_need_defaults() {
        let _ = NodeSpec {
            kind: "demo",
            inputs: vec![],
            outputs: vec![],
            params: vec![ParamSpec::int("x", 0, 9, "0")],
            gui: false,
        }
        .with_param_inputs();
    }
}
