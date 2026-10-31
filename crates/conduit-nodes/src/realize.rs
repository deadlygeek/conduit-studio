use conduit_core::{
    EvalCtx, Inputs, Node, NodeError, NodeResult, NodeSpec, Outputs, PortSpec, Value, ValueType,
    raster,
};

use crate::MAX_DIMENSION;

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
        let layer = *inputs
            .layers("layer")?
            .first()
            .ok_or_else(|| NodeError::new("input `layer` is not connected"))?;
        if let Some((x0, y0, x1, y1)) = raster::bounds(layer) {
            let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
            if w > i64::from(MAX_DIMENSION) || h > i64::from(MAX_DIMENSION) {
                return Err(NodeError::new(format!(
                    "the transformed layer would be {w}x{h}, larger than the {MAX_DIMENSION}x{MAX_DIMENSION} limit"
                )));
            }
        }
        Ok(Outputs::out(Value::Layer(raster::realize(layer))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{image, run};
    use conduit_core::{Affine, Filter, Layer};

    fn realize(layer: Layer) -> NodeResult<Layer> {
        match run(
            &Realize,
            vec![("layer", vec![Value::Layer(layer)])],
            &Default::default(),
        )? {
            Value::Layer(l) => Ok(l),
            other => panic!("expected layer, got {other:?}"),
        }
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
