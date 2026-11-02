use conduit_core::{
    EvalCtx, Inputs, Node, NodeResult, NodeSpec, Outputs, PortSpec, Rgba, Value, ValueType,
};

use crate::effect;

/// Inverts a layer's colors (a photo negative). Alpha is kept, as in Nuke and Blender.
pub struct Invert;

impl Node for Invert {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "invert",
            inputs: vec![PortSpec::new("layer", ValueType::Layer)],
            outputs: vec![PortSpec::new("out", ValueType::Layer)],
            params: vec![],
            gui: false,
        }
    }

    fn eval(&self, inputs: &Inputs, _: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let layer = effect::map_pixels(inputs, |Rgba([r, g, b, a])| {
            Rgba([255 - r, 255 - g, 255 - b, a])
        })?;
        Ok(Outputs::out(Value::Layer(layer)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::test_support::check_layer_handling;
    use crate::effect::test_support::pixel;
    use crate::test_util::run;

    #[test]
    fn inverts_color_and_keeps_alpha() {
        assert_eq!(pixel(&Invert, [255, 100, 0, 77], ""), [0, 155, 255, 77]);
    }

    #[test]
    fn handles_layers() {
        check_layer_handling(&Invert, "");
    }

    #[test]
    fn needs_a_layer() {
        let err = run(&Invert, vec![], &Default::default()).err().unwrap();
        assert!(err.0.contains("not connected"), "{err}");
    }
}
