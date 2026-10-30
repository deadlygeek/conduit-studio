//! Core types for Conduit Studio: values passed between nodes, the `Node` trait,
//! the node registry, and loading/evaluating a graph from a folder of TOML files.

pub mod color;
pub mod eval;
pub mod graph;
pub mod node;
pub mod registry;
pub mod value;

pub use color::{Color, parse_color};
pub use eval::{EvalError, Values, evaluate};
pub use graph::{Graph, GraphNode, LoadError, LoadErrors, Reference, load};
pub use node::{
    EvalCtx, GuiHost, Inputs, Node, NodeError, NodeResult, NodeSpec, NullHost, Outputs, ParamKind,
    ParamSpec, PortSpec, parse_params,
};
pub use registry::Registry;
pub use value::{Layer, Value, ValueType};

pub use image::{self, Rgba, RgbaImage};

#[cfg(test)]
mod test_nodes;
