use std::sync::Arc;

use conduit_core::{
    EvalCtx, Inputs, Node, NodeResult, NodeSpec, Outputs, PortSpec, Value, ValueType, raster,
};

/// Draws layers over a base image in order (alpha-over). The base is the layers' parent
/// canvas: each layer is resampled once, with its final transform, and clipped to it.
pub struct Composite;

impl Node for Composite {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "composite",
            inputs: vec![
                PortSpec::new("base", ValueType::Image),
                PortSpec::new("layers", ValueType::Layer).multi().optional(),
            ],
            outputs: vec![PortSpec::new("out", ValueType::Image)],
            params: vec![],
            gui: false,
        }
    }

    fn eval(&self, inputs: &Inputs, _: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let mut canvas = inputs.image("base")?.as_ref().clone();
        for layer in inputs.layers("layers")? {
            raster::draw(&mut canvas, layer, (0, 0));
        }
        Ok(Outputs::out(Value::Image(Arc::new(canvas))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conduit_core::{Affine, Layer, Rgba};

    use crate::test_util::{image, run};

    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const BLACK: [u8; 4] = [0, 0, 0, 255];

    fn layer(w: u32, h: u32, rgba: [u8; 4], x: i64, y: i64) -> Value {
        Value::Layer(Layer::at(image(w, h, rgba), x, y))
    }

    fn composite(base: Value, layers: Vec<Value>) -> Arc<conduit_core::RgbaImage> {
        let out = run(
            &Composite,
            vec![("base", vec![base]), ("layers", layers)],
            &Default::default(),
        );
        let Value::Image(img) = out.unwrap() else {
            panic!("expected image")
        };
        img
    }

    #[test]
    fn places_layer_on_base() {
        let img = composite(
            Value::Image(image(10, 10, WHITE)),
            vec![layer(2, 2, BLACK, 3, 4)],
        );
        assert_eq!(img.dimensions(), (10, 10));
        assert_eq!(img.get_pixel(3, 4).0, BLACK);
        assert_eq!(img.get_pixel(4, 5).0, BLACK);
        assert_eq!(img.get_pixel(2, 4).0, WHITE);
        assert_eq!(img.get_pixel(5, 4).0, WHITE);
    }

    #[test]
    fn later_layers_draw_on_top_and_clip() {
        let red = [255, 0, 0, 255];
        let img = composite(
            Value::Image(image(4, 4, WHITE)),
            vec![
                layer(4, 4, BLACK, -2, -2),
                layer(1, 1, red, 1, 1),
                layer(3, 3, red, 3, 3),
            ],
        );
        assert_eq!(img.get_pixel(0, 0).0, BLACK);
        assert_eq!(img.get_pixel(1, 1).0, red);
        assert_eq!(img.get_pixel(2, 2).0, WHITE);
        assert_eq!(img.get_pixel(3, 3).0, red);
    }

    #[test]
    fn blends_alpha() {
        let img = composite(
            Value::Image(image(1, 1, WHITE)),
            vec![layer(1, 1, [0, 0, 0, 128], 0, 0)],
        );
        let Rgba([r, g, b, a]) = *img.get_pixel(0, 0);
        assert_eq!(a, 255);
        assert!(
            (120..=135).contains(&r) && r == g && g == b,
            "got {r},{g},{b}"
        );
    }

    #[test]
    fn blends_onto_transparent() {
        let img = composite(
            Value::Image(image(1, 1, [0, 0, 0, 0])),
            vec![layer(1, 1, [200, 100, 50, 128], 0, 0)],
        );
        assert_eq!(img.get_pixel(0, 0).0, [200, 100, 50, 128]);
    }

    #[test]
    fn layer_fully_off_canvas() {
        let img = composite(
            Value::Image(image(2, 2, WHITE)),
            vec![layer(2, 2, BLACK, 5, 5), layer(2, 2, BLACK, -9, 0)],
        );
        assert!(img.pixels().all(|p| p.0 == WHITE));
    }

    #[test]
    fn no_layers_returns_base() {
        let base = image(3, 2, BLACK);
        let img = composite(Value::Image(base.clone()), vec![]);
        assert_eq!(*img, *base);
    }

    #[test]
    fn draws_transformed_layers() {
        // A 4×2 bar turned a quarter clockwise about its center (2, 1) stands upright,
        // covering x 1..3, y -1..3.
        let mut bar = Layer::new(image(4, 2, BLACK));
        bar.transform =
            Affine::translate(2.0, 1.0) * Affine::rotate(90.0) * Affine::translate(-2.0, -1.0);
        bar.filter = conduit_core::Filter::Nearest;
        let img = composite(Value::Image(image(4, 4, WHITE)), vec![Value::Layer(bar)]);
        let col = |x| (0..4).map(|y| img.get_pixel(x, y).0).collect::<Vec<_>>();
        assert_eq!(col(0), [WHITE; 4]);
        assert_eq!(col(1), [BLACK, BLACK, BLACK, WHITE]);
        assert_eq!(col(2), [BLACK, BLACK, BLACK, WHITE]);
        assert_eq!(col(3), [WHITE; 4]);
    }
}
