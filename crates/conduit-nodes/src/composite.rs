use std::sync::Arc;

use conduit_core::{
    EvalCtx, Inputs, Layer, Node, NodeResult, NodeSpec, Outputs, PortSpec, Rgba, RgbaImage, Value,
    ValueType,
};

/// Draws layers over a base image in order (alpha-over). Output has the base's size;
/// anything outside it is clipped.
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
            draw_layer(&mut canvas, layer);
        }
        Ok(Outputs::out(Value::Image(Arc::new(canvas))))
    }
}

/// Alpha-over of `layer` onto `canvas`, clipped to the canvas. Written by hand because
/// `image::imageops::overlay` rounds an opaque result down to alpha 254.
fn draw_layer(canvas: &mut RgbaImage, layer: &Layer) {
    let (cw, ch) = (i64::from(canvas.width()), i64::from(canvas.height()));
    let (lw, lh) = (
        i64::from(layer.image.width()),
        i64::from(layer.image.height()),
    );
    let (x0, y0) = (layer.x.max(0), layer.y.max(0));
    let (x1, y1) = ((layer.x + lw).min(cw), (layer.y + lh).min(ch));
    for y in y0..y1 {
        for x in x0..x1 {
            let src = layer
                .image
                .get_pixel((x - layer.x) as u32, (y - layer.y) as u32);
            let dst = canvas.get_pixel_mut(x as u32, y as u32);
            *dst = blend_over(*src, *dst);
        }
    }
}

fn blend_over(src: Rgba<u8>, dst: Rgba<u8>) -> Rgba<u8> {
    match src[3] {
        255 => return src,
        0 => return dst,
        _ => {}
    }
    let sa = f32::from(src[3]) / 255.0;
    let da = f32::from(dst[3]) / 255.0;
    let out_a = sa + da * (1.0 - sa);
    let channel = |i: usize| {
        let c = (f32::from(src[i]) * sa + f32::from(dst[i]) * da * (1.0 - sa)) / out_a;
        c.round() as u8
    };
    Rgba([
        channel(0),
        channel(1),
        channel(2),
        (out_a * 255.0).round() as u8,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{image, run};

    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const BLACK: [u8; 4] = [0, 0, 0, 255];

    fn layer(w: u32, h: u32, rgba: [u8; 4], x: i64, y: i64) -> Value {
        Value::Layer(Layer {
            image: image(w, h, rgba),
            x,
            y,
        })
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
}
