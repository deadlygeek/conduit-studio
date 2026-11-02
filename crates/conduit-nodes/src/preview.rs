use conduit_core::{
    EvalCtx, Inputs, Node, NodeResult, NodeSpec, Outputs, ParamSpec, PortSpec, ValueType,
    parse_params,
};
use serde::Deserialize;

/// Shows its input image in a `conduit-preview` window (GUI process managed by the runner).
pub struct Preview;

/// Declared so typos in params are rejected. The runner reads `title` from the raw params
/// when it launches the window.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    #[serde(default, rename = "title")]
    _title: Option<String>,
}

impl Node for Preview {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "preview",
            inputs: vec![PortSpec::new("image", ValueType::Image)],
            outputs: vec![],
            params: vec![
                ParamSpec::text("title", "\"Preview\"")
                    .optional()
                    .doc("window title; defaults to the node id"),
            ],
            gui: true,
        }
    }

    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        parse_params::<Params>(params).map(|_| ())
    }

    fn eval(&self, inputs: &Inputs, _: &toml::Table, ctx: &mut EvalCtx) -> NodeResult<Outputs> {
        ctx.gui.show(ctx.node_id, inputs.image("image")?.clone());
        Ok(Outputs::none())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use conduit_core::{GuiHost, RgbaImage, Value};

    use super::*;
    use crate::test_util::{image, params};

    #[derive(Default)]
    struct Recorder(Vec<(String, (u32, u32))>);

    impl GuiHost for Recorder {
        fn show(&mut self, node_id: &str, image: Arc<RgbaImage>) {
            self.0.push((node_id.to_string(), image.dimensions()));
        }
    }

    #[test]
    fn sends_image_to_gui_host() {
        let inputs =
            HashMap::from([("image".to_string(), vec![Value::Image(image(4, 3, [0; 4]))])]);
        let mut gui = Recorder::default();
        let mut ctx = EvalCtx {
            node_id: "prev",
            root: std::path::Path::new(""),
            gui: &mut gui,
        };
        let out = Preview
            .eval(&Inputs::new(inputs), &params(""), &mut ctx)
            .unwrap();
        assert!(out.values.is_empty());
        assert_eq!(gui.0, [("prev".to_string(), (4, 3))]);
    }

    #[test]
    fn title_param() {
        assert!(Preview.check_params(&params("title = \"Main\"")).is_ok());
        assert!(Preview.check_params(&params("size = 1")).is_err());
    }
}
