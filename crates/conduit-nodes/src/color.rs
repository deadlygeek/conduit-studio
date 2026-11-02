use conduit_core::{
    Color, EvalCtx, Inputs, Node, NodeResult, NodeSpec, Outputs, ParamSpec, PortSpec, Value,
    ValueType, adjust,
};
use serde::Deserialize;

/// A color: a base (typed, or linked from a palette or another color) with the adjustments
/// image editors offer — hue rotation, saturation, lightness and opacity.
pub struct ColorNode;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    color: Color,
    hue: i64,
    saturation: i64,
    lightness: i64,
    alpha: i64,
}

impl Node for ColorNode {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "color",
            inputs: vec![],
            outputs: vec![PortSpec::new("out", ValueType::Color)],
            params: vec![
                ParamSpec::color("color", "\"#e63946\"").doc("base color"),
                ParamSpec::int("hue", -180, 180, "0")
                    .default("0")
                    .doc("rotate the hue, in degrees"),
                ParamSpec::int("saturation", -100, 100, "0")
                    .default("0")
                    .doc("more (+) or less (-) saturated, in %"),
                ParamSpec::int("lightness", -100, 100, "0")
                    .default("0")
                    .doc("lighter (+) or darker (-), in %"),
                ParamSpec::int("alpha", 0, 100, "100")
                    .default("100")
                    .doc("opacity, in % of the base's"),
            ],
            gui: false,
        }
    }

    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        self.spec().parse_params::<Params>(params).map(|_| ())
    }

    fn eval(&self, _: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let p: Params = self.spec().parse_params(params)?;
        let c = adjust(
            p.color.0,
            p.hue as f64,
            p.saturation as f64,
            p.lightness as f64,
            p.alpha as f64,
        );
        Ok(Outputs::out(Value::Color(c)))
    }
}

#[cfg(test)]
mod tests {
    use conduit_core::Rgba;

    use super::*;
    use crate::test_util::{params, run};

    fn color(p: &str) -> Rgba<u8> {
        match run(&ColorNode, vec![], &params(p)).unwrap() {
            Value::Color(c) => c,
            other => panic!("expected color, got {other:?}"),
        }
    }

    #[test]
    fn base_and_adjustments() {
        assert_eq!(color("color = \"#264653\""), Rgba([0x26, 0x46, 0x53, 255]));
        assert_eq!(color("color = \"red\"\nhue = 120"), Rgba([0, 255, 0, 255]));
        assert_eq!(color("color = \"red\"\nlightness = 50"), Rgba([255; 4]));
        assert_eq!(color("color = \"red\"\nalpha = 50"), Rgba([255, 0, 0, 128]));
    }

    #[test]
    fn rejects_out_of_range() {
        assert!(
            ColorNode
                .check_params(&params("color = \"red\"\nhue = 181"))
                .is_err()
        );
        assert!(
            ColorNode
                .check_params(&params("color = \"red\"\nalpha = -1"))
                .is_err()
        );
        assert!(
            ColorNode.check_params(&params("hue = 1")).is_err(),
            "color required"
        );
    }
}
