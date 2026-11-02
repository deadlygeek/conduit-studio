use conduit_core::{
    EvalCtx, Inputs, Node, NodeResult, NodeSpec, Outputs, PortSpec, Value, ValueType,
};

use crate::effect;

/// Renders a transformed layer onto its own bounding box, so pixel effects can follow a
/// transform without clipping to a canvas. The result is a layer moved by whole pixels:
/// its image holds the transformed pixels and it still lands in the same place.
pub struct Realize;

impl Node for Realize {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "realize",
            inputs: vec![PortSpec::new("layer", ValueType::Layer)],
            outputs: vec![PortSpec::new("out", ValueType::Layer)],
            params: vec![],
            gui: false,
        }
    }

    fn eval(&self, inputs: &Inputs, _: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let layer = effect::realized(effect::input_layer(inputs)?)?;
        Ok(Outputs::out(Value::Layer(layer)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{image, on_layer};
    use conduit_core::{Affine, Filter, Layer};

    fn realize(layer: Layer) -> NodeResult<Layer> {
        on_layer(&Realize, layer, &Default::default())
    }

    #[test]
    fn bakes_the_transform_into_pixels() {
        let mut layer = Layer::new(image(4, 2, [0, 0, 0, 255]));
        layer.transform = Affine::translate(10.0, 0.0) * Affine::rotate(90.0);
        layer.filter = Filter::Nearest;
        let out = realize(layer).unwrap();
        assert_eq!(out.image.dimensions(), (2, 4));
        assert_eq!(out.transform.whole_pixel_offset(), Some((8, 0)));
        assert_eq!(out.filter, Filter::Nearest);
    }

    #[test]
    fn rejects_huge_results() {
        let mut layer = Layer::new(image(2, 2, [0; 4]));
        layer.transform = Affine::scale(100_000.0, 1.5);
        let err = realize(layer).err().unwrap();
        assert!(err.0.contains("larger than the 16384x16384 limit"), "{err}");
    }
}
