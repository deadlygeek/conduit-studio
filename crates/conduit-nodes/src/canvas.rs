//! `canvas`: a background — a connected image, or a solid fill of a chosen size — at an
//! opacity.

use std::sync::Arc;

use conduit_core::{
    Color, EvalCtx, Inputs, Node, NodeResult, NodeSpec, Outputs, ParamSpec, PortSpec, RgbaImage,
    Value, ValueType,
};
use serde::Deserialize;

use crate::MAX_DIMENSION;

/// A background image: `image` when connected, else a `width` × `height` fill of `color`.
/// `opacity` applies either way.
pub struct Canvas;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    color: Color,
    width: u32,
    height: u32,
    opacity: u8,
}

impl Node for Canvas {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "canvas",
            inputs: vec![PortSpec::new("image", ValueType::Image).optional()],
            outputs: vec![PortSpec::new("out", ValueType::Image)],
            params: vec![
                ParamSpec::color("color", "\"white\"")
                    .default("\"white\"")
                    .unless_connected("image")
                    .doc("fill color, without an image"),
                ParamSpec::int("width", 1, MAX_DIMENSION.into(), "1920")
                    .default("1920")
                    .unless_connected("image")
                    .doc("width in pixels, without an image"),
                ParamSpec::int("height", 1, MAX_DIMENSION.into(), "1080")
                    .default("1080")
                    .unless_connected("image")
                    .doc("height in pixels, without an image"),
                ParamSpec::int("opacity", 0, 100, "100")
                    .default("100")
                    .doc("opacity, in %"),
            ],
            gui: false,
        }
    }

    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        self.spec().parse_params::<Params>(params).map(|_| ())
    }

    fn eval(&self, inputs: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let p: Params = self.spec().parse_params(params)?;
        let mut image = match inputs.get("image") {
            Some(_) => inputs.image("image")?.as_ref().clone(),
            None => RgbaImage::from_pixel(p.width, p.height, p.color.0),
        };
        if p.opacity < 100 {
            let scale = f64::from(p.opacity) / 100.0;
            for px in image.pixels_mut() {
                px[3] = (f64::from(px[3]) * scale).round() as u8;
            }
        }
        Ok(Outputs::out(Value::Image(Arc::new(image))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{image, params, run};

    fn canvas(inputs: Vec<(&str, Vec<Value>)>, toml_src: &str) -> Arc<RgbaImage> {
        let Value::Image(img) = run(&Canvas, inputs, &params(toml_src)).unwrap() else {
            panic!("expected image")
        };
        img
    }

    #[test]
    fn fills_with_color_and_size() {
        let img = canvas(vec![], "color = \"#ff0000\"\nwidth = 4\nheight = 2");
        assert_eq!(img.dimensions(), (4, 2));
        assert!(img.pixels().all(|p| p.0 == [255, 0, 0, 255]));
    }

    #[test]
    fn defaults_to_white_1920_by_1080() {
        let img = canvas(vec![], "");
        assert_eq!(img.dimensions(), (1920, 1080));
        assert_eq!(img.get_pixel(0, 0).0, [255, 255, 255, 255]);
    }

    #[test]
    fn a_connected_image_replaces_color_and_size() {
        let src = image(3, 5, [10, 20, 30, 255]);
        let img = canvas(
            vec![("image", vec![Value::Image(src.clone())])],
            "color = \"#ff0000\"\nwidth = 4\nheight = 2",
        );
        assert_eq!(*img, *src);
    }

    #[test]
    fn opacity_scales_alpha_either_way() {
        let filled = canvas(vec![], "width = 1\nheight = 1\nopacity = 50");
        assert_eq!(filled.get_pixel(0, 0).0, [255, 255, 255, 128]);
        let src = image(1, 1, [10, 20, 30, 200]);
        let img = canvas(vec![("image", vec![Value::Image(src)])], "opacity = 50");
        assert_eq!(img.get_pixel(0, 0).0, [10, 20, 30, 100]);
        let clear = canvas(vec![], "width = 2\nheight = 2\nopacity = 0");
        assert!(clear.pixels().all(|p| p[3] == 0));
    }

    #[test]
    fn rejects_bad_params() {
        let check = |s: &str| Canvas.check_params(&params(s));
        assert!(check("").is_ok(), "everything defaults");
        assert!(check("width = 0").is_err());
        assert!(check("height = 99999").is_err());
        assert!(check("opacity = 101").is_err());
        assert!(check("opacity = -1").is_err());
        assert!(check("color = \"nope\"").is_err());
        assert!(check("size = 3").is_err());
    }
}
