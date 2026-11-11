//! Vector shape nodes: `rectangle` and `oval` make shapes, `union`, `subtract`,
//! `intersect` and `exclude` combine them, and `rasterize` turns one into pixels. A shape
//! also becomes a layer by itself when wired into a layer input.

use std::sync::Arc;

use conduit_core::{
    BooleanOp, Color, EvalCtx, Inputs, Layer, Node, NodeError, NodeResult, NodeSpec, Outputs,
    ParamSpec, PortSpec, Shape, Style, Value, ValueType, shape,
};
use serde::Deserialize;

use crate::MAX_DIMENSION;

/// Widest stroke allowed, in pixels.
const MAX_STROKE: f64 = 1000.0;

/// The params every shape node shares: where it sits and how it is painted.
fn common_params() -> Vec<ParamSpec> {
    let any = |name, example| ParamSpec::float(name, f64::NEG_INFINITY, f64::INFINITY, example);
    vec![
        any("x", "0")
            .default("0")
            .doc("left edge before rotating, in parent pixels"),
        any("y", "0")
            .default("0")
            .doc("top edge before rotating, in parent pixels"),
        any("rotate", "0")
            .default("0")
            .doc("degrees clockwise, about the shape's centre"),
        ParamSpec::color("fill_color", "\"white\"")
            .default("\"white\"")
            .doc("fill color (\"transparent\" for none)"),
        ParamSpec::int("fill_opacity", 0, 100, "100")
            .default("100")
            .doc("fill opacity, in %"),
        ParamSpec::float("stroke_width", 0.0, MAX_STROKE, "0")
            .default("0")
            .doc("outline thickness in pixels, centred on the edge; 0 for none"),
        ParamSpec::color("stroke_color", "\"black\"")
            .default("\"black\"")
            .doc("outline color"),
        ParamSpec::int("stroke_opacity", 0, 100, "100")
            .default("100")
            .doc("outline opacity, in %"),
    ]
}

fn size_params() -> Vec<ParamSpec> {
    let max = f64::from(MAX_DIMENSION);
    vec![
        ParamSpec::float("width", 0.0, max, "100")
            .default("100")
            .doc("width in pixels"),
        ParamSpec::float("height", 0.0, max, "100")
            .default("100")
            .doc("height in pixels"),
    ]
}

/// Fields shared by the shape params structs, gathered so placing and painting is written
/// once.
struct Common {
    x: f64,
    y: f64,
    rotate: f64,
    width: f64,
    height: f64,
    style: Style,
}

macro_rules! shape_params {
    ($name:ident { $($extra:ident: $ty:ty),* }) => {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct $name {
            x: f64,
            y: f64,
            rotate: f64,
            width: f64,
            height: f64,
            fill_color: Color,
            fill_opacity: u8,
            stroke_width: f64,
            stroke_color: Color,
            stroke_opacity: u8,
            $($extra: $ty,)*
        }

        impl $name {
            fn common(&self) -> NodeResult<Common> {
                for (name, v) in [("x", self.x), ("y", self.y), ("rotate", self.rotate)] {
                    if !v.is_finite() {
                        return Err(NodeError::new(format!("`{name}` must be a finite number")));
                    }
                }
                Ok(Common {
                    x: self.x,
                    y: self.y,
                    rotate: self.rotate,
                    width: self.width,
                    height: self.height,
                    style: Style {
                        fill: self.fill_color.0,
                        fill_opacity: f64::from(self.fill_opacity) / 100.0,
                        stroke: self.stroke_color.0,
                        stroke_opacity: f64::from(self.stroke_opacity) / 100.0,
                        stroke_width: self.stroke_width,
                    },
                })
            }
        }
    };
}

shape_params!(RectangleParams { corner_radius: f64 });
shape_params!(OvalParams {});

impl Common {
    /// `shape` (drawn in its `width` × `height` box at the origin) moved and rotated into
    /// place.
    fn place(&self, shape: Shape) -> NodeResult<Outputs> {
        let placed = shape.placed(self.x, self.y, self.rotate, self.width, self.height);
        output(placed)
    }
}

/// `shape` as a node output, once it is known to fit when rasterized.
fn output(shape: Shape) -> NodeResult<Outputs> {
    if let Some((x0, y0, x1, y1)) = shape.raster_bounds() {
        let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        let max = i64::from(MAX_DIMENSION);
        if w > max || h > max {
            return Err(NodeError::new(format!(
                "the shape would be {w}x{h} pixels, larger than the {max}x{max} limit"
            )));
        }
    }
    Ok(Outputs::out(Value::Shape(shape)))
}

/// A rectangle, optionally with rounded corners.
pub struct Rectangle;

impl Node for Rectangle {
    fn spec(&self) -> NodeSpec {
        let mut params = size_params();
        params.push(
            ParamSpec::float("corner_radius", 0.0, f64::from(MAX_DIMENSION), "0")
                .default("0")
                .doc("rounds the corners, in pixels (at most half the shorter side)"),
        );
        params.extend(common_params());
        NodeSpec {
            kind: "rectangle",
            inputs: vec![],
            outputs: vec![PortSpec::new("out", ValueType::Shape)],
            params,
            gui: false,
        }
    }

    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        let p: RectangleParams = self.spec().parse_params(params)?;
        p.common().map(|_| ())
    }

    fn eval(&self, _: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let p: RectangleParams = self.spec().parse_params(params)?;
        let c = p.common()?;
        c.place(Shape::rectangle(
            c.width,
            c.height,
            p.corner_radius,
            c.style,
        ))
    }
}

/// An oval (ellipse) filling a `width` × `height` box; equal sides make a circle.
pub struct Oval;

impl Node for Oval {
    fn spec(&self) -> NodeSpec {
        let mut params = size_params();
        params[0] = params[0].clone().doc("horizontal diameter in pixels");
        params[1] = params[1].clone().doc("vertical diameter in pixels");
        params.extend(common_params());
        NodeSpec {
            kind: "oval",
            inputs: vec![],
            outputs: vec![PortSpec::new("out", ValueType::Shape)],
            params,
            gui: false,
        }
    }

    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        let p: OvalParams = self.spec().parse_params(params)?;
        p.common().map(|_| ())
    }

    fn eval(&self, _: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let p: OvalParams = self.spec().parse_params(params)?;
        let c = p.common()?;
        c.place(Shape::oval(c.width, c.height, c.style))
    }
}

/// A boolean operation over a list of shapes, applied left to right. The result is painted
/// like the first shape, with its stroke following the combined outline.
pub struct Boolean {
    kind: &'static str,
    op: BooleanOp,
}

impl Boolean {
    /// Everything covered by any of the shapes.
    pub const UNION: Self = Self {
        kind: "union",
        op: BooleanOp::Union,
    };
    /// `base` with every `cut` shape removed from it.
    pub const SUBTRACT: Self = Self {
        kind: "subtract",
        op: BooleanOp::Subtract,
    };
    /// Only where all the shapes overlap.
    pub const INTERSECT: Self = Self {
        kind: "intersect",
        op: BooleanOp::Intersect,
    };
    /// Where an odd number of shapes overlap: two shapes lose their shared part.
    pub const EXCLUDE: Self = Self {
        kind: "exclude",
        op: BooleanOp::Exclude,
    };
}

impl Node for Boolean {
    fn spec(&self) -> NodeSpec {
        let inputs = if self.op == BooleanOp::Subtract {
            vec![
                PortSpec::new("base", ValueType::Shape),
                PortSpec::new("cut", ValueType::Shape).multi(),
            ]
        } else {
            vec![PortSpec::new("shapes", ValueType::Shape).multi()]
        };
        NodeSpec {
            kind: self.kind,
            inputs,
            outputs: vec![PortSpec::new("out", ValueType::Shape)],
            params: vec![],
            gui: false,
        }
    }

    fn eval(&self, inputs: &Inputs, _: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let result = if self.op == BooleanOp::Subtract {
            let base = inputs.shape("base")?;
            // Cutting each in turn equals cutting their union.
            inputs
                .shapes("cut")?
                .into_iter()
                .fold(base.clone(), |acc, cut| {
                    acc.combine(cut, BooleanOp::Subtract)
                })
        } else {
            let shapes = inputs.shapes("shapes")?;
            let Some((first, rest)) = shapes.split_first() else {
                return Err(NodeError::new("input `shapes` needs at least one shape"));
            };
            rest.iter()
                .fold((*first).clone(), |acc, s| acc.combine(s, self.op))
        };
        output(result)
    }
}

/// Paints a shape into pixels on its own bounding box, placed where the shape is. Wiring a
/// shape into any layer input does the same; this node makes the step visible.
pub struct Rasterize;

impl Node for Rasterize {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "rasterize",
            inputs: vec![PortSpec::new("shape", ValueType::Shape)],
            outputs: vec![PortSpec::new("out", ValueType::Layer)],
            params: vec![],
            gui: false,
        }
    }

    fn eval(&self, inputs: &Inputs, _: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let (image, x, y) = shape::rasterize(inputs.shape("shape")?);
        Ok(Outputs::out(Value::Layer(Layer::at(Arc::new(image), x, y))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{params, run};

    const RED: [u8; 4] = [255, 0, 0, 255];
    const CLEAR: [u8; 4] = [0; 4];

    fn shape_of(node: &dyn Node, toml: &str) -> Shape {
        match run(node, vec![], &params(toml)).unwrap() {
            Value::Shape(s) => s,
            other => panic!("expected shape, got {other:?}"),
        }
    }

    fn combined(node: &Boolean, inputs: Vec<(&str, Vec<Shape>)>) -> NodeResult<Shape> {
        let inputs = inputs
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().map(Value::Shape).collect()))
            .collect();
        match run(node, inputs, &Default::default())? {
            Value::Shape(s) => Ok(s),
            other => panic!("expected shape, got {other:?}"),
        }
    }

    /// The color at parent pixel (x, y) of the shape as a layer.
    fn pixel(shape: &Shape, x: i64, y: i64) -> [u8; 4] {
        let layer = match run(
            &Rasterize,
            vec![("shape", vec![Value::Shape(shape.clone())])],
            &Default::default(),
        )
        .unwrap()
        {
            Value::Layer(l) => l,
            other => panic!("expected layer, got {other:?}"),
        };
        let (lx, ly) = layer.transform.whole_pixel_offset().unwrap();
        let (px, py) = (x - lx, y - ly);
        let (w, h) = layer.image.dimensions();
        if px < 0 || py < 0 || px >= i64::from(w) || py >= i64::from(h) {
            return CLEAR;
        }
        layer.image.get_pixel(px as u32, py as u32).0
    }

    fn square(x: i64, y: i64, color: &str) -> Shape {
        shape_of(
            &Rectangle,
            &format!("x = {x}\ny = {y}\nwidth = 10\nheight = 10\nfill_color = \"{color}\""),
        )
    }

    #[test]
    fn rectangle_lands_at_x_y_with_its_fill() {
        let s = square(5, 7, "red");
        assert_eq!(s.raster_bounds(), Some((5, 7, 15, 17)));
        assert_eq!(pixel(&s, 5, 7), RED);
        assert_eq!(pixel(&s, 14, 16), RED);
        assert_eq!(pixel(&s, 15, 16), CLEAR);
    }

    #[test]
    fn defaults_make_a_white_100px_square_at_the_origin() {
        let s = shape_of(&Rectangle, "");
        assert_eq!(s.raster_bounds(), Some((0, 0, 100, 100)));
        assert_eq!(s.style, Style::default());
    }

    #[test]
    fn rotation_turns_about_the_centre() {
        let s = shape_of(
            &Rectangle,
            "x = 0\ny = 0\nwidth = 40\nheight = 10\nrotate = 90",
        );
        assert_eq!(s.raster_bounds(), Some((15, -15, 25, 25)));
    }

    #[test]
    fn opacities_are_percentages() {
        let s = shape_of(
            &Oval,
            "fill_opacity = 50\nstroke_opacity = 25\nstroke_width = 2",
        );
        assert_eq!(s.style.fill_opacity, 0.5);
        assert_eq!(s.style.stroke_opacity, 0.25);
        assert_eq!(pixel(&s, 50, 50), [255, 255, 255, 128]);
    }

    #[test]
    fn oval_corners_are_clear() {
        let s = shape_of(&Oval, "width = 20\nheight = 10\nfill_color = \"red\"");
        assert_eq!(pixel(&s, 10, 5), RED);
        assert_eq!(pixel(&s, 0, 0), CLEAR);
    }

    #[test]
    fn bad_params_are_rejected() {
        let check = |n: &dyn Node, s: &str| n.check_params(&params(s));
        assert!(check(&Rectangle, "width = -1").is_err());
        assert!(check(&Rectangle, "fill_opacity = 101").is_err());
        assert!(check(&Rectangle, "fill_color = \"nope\"").is_err());
        assert!(check(&Rectangle, "x = inf").is_err());
        assert!(
            check(&Oval, "corner_radius = 3").is_err(),
            "ovals have no corners"
        );
        assert!(check(&Rectangle, "corner_radius = 3").is_ok());
    }

    #[test]
    fn huge_shapes_are_rejected() {
        let err = run(
            &Rectangle,
            vec![],
            &params("width = 16384\nheight = 16384\nrotate = 45"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("limit"), "{err}");
    }

    #[test]
    fn union_covers_both_and_keeps_the_first_style() {
        let s = combined(
            &Boolean::UNION,
            vec![("shapes", vec![square(0, 0, "red"), square(5, 0, "blue")])],
        )
        .unwrap();
        assert_eq!(s.raster_bounds(), Some((0, 0, 15, 10)));
        assert_eq!(pixel(&s, 12, 5), RED);
    }

    #[test]
    fn subtract_removes_every_cut() {
        let s = combined(
            &Boolean::SUBTRACT,
            vec![
                ("base", vec![square(0, 0, "red")]),
                ("cut", vec![square(-5, 0, "blue"), square(5, 0, "blue")]),
            ],
        )
        .unwrap();
        assert!(s.contours.is_empty());
    }

    #[test]
    fn intersect_and_exclude() {
        let pair = || vec![("shapes", vec![square(0, 0, "red"), square(5, 0, "blue")])];
        let both = combined(&Boolean::INTERSECT, pair()).unwrap();
        assert_eq!(both.raster_bounds(), Some((5, 0, 10, 10)));
        let either = combined(&Boolean::EXCLUDE, pair()).unwrap();
        assert_eq!(pixel(&either, 7, 5), CLEAR);
        assert_eq!(pixel(&either, 2, 5), RED);
        assert_eq!(pixel(&either, 12, 5), RED);
    }

    #[test]
    fn one_shape_passes_through_and_none_is_an_error() {
        let s = combined(&Boolean::UNION, vec![("shapes", vec![square(0, 0, "red")])]).unwrap();
        assert_eq!(s, square(0, 0, "red"));
        assert!(combined(&Boolean::INTERSECT, vec![("shapes", vec![])]).is_err());
    }
}
