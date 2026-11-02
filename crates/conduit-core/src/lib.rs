//! Core types for Conduit Studio: values passed between nodes, the `Node` trait,
//! the node registry, and loading/evaluating a graph from a folder of TOML files.

pub mod affine;
pub mod color;
pub mod edit;
pub mod eval;
pub mod graph;
pub mod node;
pub mod palette;
pub mod project;
pub mod query;
pub mod raster;
pub mod registry;
pub mod value;

pub use affine::{Affine, Filter};
pub use color::{
    Channel, Color, ColorModel, Hsl, adjust, from_hsl, luminance, parse_color, to_hex, to_hsl,
};
pub use edit::{Change, EditError, EditResult};
pub use eval::{EvalError, EvalStats, Evaluator, Values, evaluate};
pub use graph::{Graph, GraphNode, LoadError, LoadErrors, Reference, load};
pub use node::{
    EvalCtx, GuiHost, Inputs, Node, NodeError, NodeResult, NodeSpec, NullHost, Outputs, ParamKind,
    ParamSpec, PortSpec, float_value, parse_literal, parse_params,
};
pub use palette::parse_palette;
pub use project::{Consumer, Project, ProjectNode, inspect, inspect_sources};
pub use query::TreeItem;
pub use registry::Registry;
pub use value::{FontData, Layer, Value, ValueType};

pub use image::{self, Rgba, RgbaImage};

#[cfg(test)]
mod test_nodes;
