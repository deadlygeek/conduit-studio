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
    use crate::test_util::{image, run};

    #[test]
    fn inverts_color_and_keeps_alpha() {
        let out = run(
            &Invert,
            vec![(
                "layer",
                vec![Value::Layer(conduit_core::Layer::new(image(
                    2,
                    2,
                    [255, 100, 0, 77],
                )))],
            )],
            &Default::default(),
        );
        match out {
            Ok(Value::Layer(l)) => assert_eq!(l.image.get_pixel(1, 1).0, [0, 155, 255, 77]),
            other => panic!("expected layer, got {other:?}"),
        }
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
