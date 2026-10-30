use std::sync::Arc;

use conduit_core::{
    Color, EvalCtx, Inputs, Node, NodeError, NodeResult, NodeSpec, Outputs, ParamSpec, PortSpec,
    RgbaImage, Value, ValueType, parse_params,
};
use serde::Deserialize;

use crate::MAX_DIMENSION;

/// A solid-colored rectangle image.
pub struct Rect;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    color: Color,
    width: u32,
    height: u32,
}

impl Params {
    fn parse(params: &toml::Table) -> NodeResult<Self> {
        let p: Self = parse_params(params)?;
        for (name, v) in [("width", p.width), ("height", p.height)] {
            if v == 0 || v > MAX_DIMENSION {
                return Err(NodeError::new(format!(
                    "`{name}` must be between 1 and {MAX_DIMENSION}, got {v}"
                )));
            }
        }
        Ok(p)
    }
}

impl Node for Rect {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "rect",
            inputs: vec![],
            outputs: vec![PortSpec::new("out", ValueType::Image)],
            params: vec![
                ParamSpec::color("color", "\"white\"").doc("fill color"),
                ParamSpec::int("width", 1, MAX_DIMENSION.into(), "100").doc("width in pixels"),
                ParamSpec::int("height", 1, MAX_DIMENSION.into(), "100").doc("height in pixels"),
            ],
            gui: false,
        }
    }

    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        Params::parse(params).map(|_| ())
    }

    fn eval(&self, _: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let p = Params::parse(params)?;
        let image = RgbaImage::from_pixel(p.width, p.height, p.color.0);
        Ok(Outputs::out(Value::Image(Arc::new(image))))
    }
}

#[cfg(test)]
mod tests {
    use conduit_core::Rgba;

    use super::*;
    use crate::test_util::{params, run};

    #[test]
    fn fills_with_color() {
        let out = run(
            &Rect,
            vec![],
            &params(
                r##"color = "#ff000080"
width = 4
height = 2"##,
            ),
        )
        .unwrap();
        let Value::Image(img) = out else {
            panic!("expected image")
        };
        assert_eq!(img.dimensions(), (4, 2));
        assert!(img.pixels().all(|p| *p == Rgba([255, 0, 0, 128])));
    }

    #[test]
    fn rejects_bad_params() {
        let check = |s: &str| Rect.check_params(&params(s));
        assert!(check("color = \"white\"\nwidth = 0\nheight = 5").is_err());
        assert!(check("color = \"white\"\nwidth = 99999\nheight = 5").is_err());
        assert!(check("color = \"nope\"\nwidth = 5\nheight = 5").is_err());
        assert!(check("color = \"white\"\nwidth = -1\nheight = 5").is_err());
        assert!(check("color = \"white\"\nwidth = 5").is_err());
        assert!(check("color = \"white\"\nwidth = 5\nheight = 5").is_ok());
    }
}
