use conduit_core::{
    Affine, EvalCtx, Filter, Inputs, Layer, Node, NodeError, NodeResult, NodeSpec, Outputs,
    ParamSpec, PortSpec, Value, ValueType,
};
use serde::Deserialize;

/// Moves, rotates, scales and skews a layer within its parent canvas. Only the layer's
/// transform changes; pixels are resampled once, when the layer is drawn or realized, so
/// chained transforms never blur twice.
pub struct Transform;

/// Steepest skew allowed, in degrees; 90 would flatten the layer into a line.
const MAX_SKEW: f64 = 89.0;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    x: f64,
    y: f64,
    rotate: f64,
    scale: f64,
    scale_x: f64,
    scale_y: f64,
    skew_x: f64,
    skew_y: f64,
    anchor_x: f64,
    anchor_y: f64,
    filter: Option<Filter>,
}

impl Params {
    /// Also rejects infinities, which the open ranges of most params let through.
    fn parse(params: &toml::Table) -> NodeResult<Self> {
        let p: Params = Transform.spec().parse_params(params)?;
        let numbers = [
            ("x", p.x),
            ("y", p.y),
            ("rotate", p.rotate),
            ("scale", p.scale),
            ("scale_x", p.scale_x),
            ("scale_y", p.scale_y),
            ("anchor_x", p.anchor_x),
            ("anchor_y", p.anchor_y),
            ("skew_x", p.skew_x),
            ("skew_y", p.skew_y),
        ];
        for (name, v) in numbers {
            if !v.is_finite() {
                return Err(NodeError::new(format!("`{name}` must be a finite number")));
            }
        }
        Ok(p)
    }

    /// The transform to apply on top of `layer`'s: rotate, skew and scale about the
    /// anchor's current position in the parent, then move by (x, y).
    fn matrix(&self, layer: &Layer) -> Affine {
        let (w, h) = layer.image.dimensions();
        let (px, py) = layer
            .transform
            .apply(self.anchor_x * f64::from(w), self.anchor_y * f64::from(h));
        Affine::translate(self.x + px, self.y + py)
            * Affine::rotate(self.rotate)
            * Affine::skew(self.skew_x, self.skew_y)
            * Affine::scale(self.scale * self.scale_x, self.scale * self.scale_y)
            * Affine::translate(-px, -py)
    }
}

impl Node for Transform {
    fn spec(&self) -> NodeSpec {
        let any = |name, example| ParamSpec::float(name, f64::NEG_INFINITY, f64::INFINITY, example);
        NodeSpec {
            kind: "transform",
            inputs: vec![PortSpec::new("layer", ValueType::Layer)],
            outputs: vec![PortSpec::new("out", ValueType::Layer)],
            params: vec![
                any("x", "0")
                    .default("0")
                    .doc("move right, in parent pixels (an image's left edge lands here)"),
                any("y", "0")
                    .default("0")
                    .doc("move down, in parent pixels (an image's top edge lands here)"),
                any("rotate", "0")
                    .default("0")
                    .doc("degrees clockwise, about the anchor"),
                any("scale", "1")
                    .default("1")
                    .doc("size multiplier, about the anchor"),
                any("scale_x", "1")
                    .default("1")
                    .doc("extra horizontal multiplier (negative flips)"),
                any("scale_y", "1")
                    .default("1")
                    .doc("extra vertical multiplier (negative flips)"),
                ParamSpec::float("skew_x", -MAX_SKEW, MAX_SKEW, "0")
                    .default("0")
                    .doc("slant vertical edges, in degrees"),
                ParamSpec::float("skew_y", -MAX_SKEW, MAX_SKEW, "0")
                    .default("0")
                    .doc("slant horizontal edges, in degrees"),
                any("anchor_x", "0.5")
                    .default("0.5")
                    .doc("pivot across the image: 0 left edge, 1 right edge"),
                any("anchor_y", "0.5")
                    .default("0.5")
                    .doc("pivot down the image: 0 top edge, 1 bottom edge"),
                ParamSpec::text("filter", "\"bilinear\"")
                    .optional()
                    .doc("nearest, bilinear or bicubic; unset keeps the incoming layer's"),
            ],
            gui: false,
        }
    }

    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        Params::parse(params).map(|_| ())
    }

    fn eval(&self, inputs: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let p = Params::parse(params)?;
        let layer = inputs.layer("layer")?;
        Ok(Outputs::out(Value::Layer(Layer {
            image: layer.image.clone(),
            transform: p.matrix(layer) * layer.transform,
            filter: p.filter.unwrap_or(layer.filter),
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{image, params, run_layer};

    fn transform(input: Value, toml: &str) -> Layer {
        run_layer(&Transform, vec![("layer", vec![input])], &params(toml)).unwrap()
    }

    /// An image as the evaluator delivers it to a layer input.
    fn img(w: u32, h: u32) -> Value {
        Value::Layer(Layer::new(image(w, h, [0, 0, 0, 255])))
    }

    fn close(p: (f64, f64), q: (f64, f64)) {
        assert!(
            (p.0 - q.0).abs() < 1e-9 && (p.1 - q.1).abs() < 1e-9,
            "{p:?} != {q:?}"
        );
    }

    #[test]
    fn moves_an_image_by_whole_pixels() {
        // Whole-number params in the file are fine for float params.
        let layer = transform(img(3, 3), "x = -5\ny = 12");
        assert_eq!(layer.transform.whole_pixel_offset(), Some((-5, 12)));
        assert_eq!(layer.filter, Filter::Bilinear);
    }

    #[test]
    fn defaults_change_nothing_and_unknown_params_are_rejected() {
        let layer = transform(img(1, 1), "");
        assert_eq!(layer.transform, Affine::IDENTITY);
        assert!(Transform.check_params(&params("z = 1")).is_err());
        assert!(
            Transform
                .check_params(&params("filter = \"blurry\""))
                .is_err()
        );
        assert!(Transform.check_params(&params("skew_x = 89.5")).is_err());
        assert!(Transform.check_params(&params("rotate = nan")).is_err());
    }

    #[test]
    fn rotates_about_the_image_center() {
        let layer = transform(img(4, 2), "rotate = 90");
        // The center stays put; the top-left corner swings to the top-right.
        close(layer.transform.apply(2.0, 1.0), (2.0, 1.0));
        close(layer.transform.apply(0.0, 0.0), (3.0, -1.0));
    }

    #[test]
    fn anchor_moves_the_pivot() {
        let layer = transform(img(4, 2), "scale = 2\nanchor_x = 0\nanchor_y = 0");
        close(layer.transform.apply(0.0, 0.0), (0.0, 0.0));
        close(layer.transform.apply(4.0, 2.0), (8.0, 4.0));
    }

    #[test]
    fn chained_transforms_pivot_where_the_layer_is_now() {
        // Move first, then rotate: the rotation happens around the moved center.
        let moved = transform(img(4, 2), "x = 100");
        let turned = transform(Value::Layer(moved), "rotate = 90");
        close(turned.transform.apply(2.0, 1.0), (102.0, 1.0));
        close(turned.transform.apply(0.0, 0.0), (103.0, -1.0));
        // Translation is always in the parent's pixels, even after a rotation.
        let shifted = transform(Value::Layer(turned), "x = 10");
        close(shifted.transform.apply(2.0, 1.0), (112.0, 1.0));
    }

    #[test]
    fn last_filter_wins_and_unset_keeps_it() {
        let nearest = transform(img(1, 1), "filter = \"nearest\"");
        let kept = transform(Value::Layer(nearest.clone()), "rotate = 10");
        assert_eq!(kept.filter, Filter::Nearest);
        let changed = transform(Value::Layer(nearest), "filter = \"bicubic\"");
        assert_eq!(changed.filter, Filter::Bicubic);
    }
}
