use conduit_core::{
    EvalCtx, Inputs, Node, NodeError, NodeResult, NodeSpec, Outputs, ParamSpec, PortSpec, Value,
    ValueType, adjust, parse_params,
};
use serde::Deserialize;

use crate::effect;

/// Hue / saturation / lightness: the `color` node's adjustments, applied to every pixel
/// of a layer.
pub struct Hsl;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    #[serde(default)]
    hue: i64,
    #[serde(default)]
    saturation: i64,
    #[serde(default)]
    lightness: i64,
    #[serde(default = "full")]
    alpha: i64,
}

fn full() -> i64 {
    100
}

impl Params {
    fn parse(params: &toml::Table) -> NodeResult<Self> {
        let p: Self = parse_params(params)?;
        for (name, v, min, max) in [
            ("hue", p.hue, -180, 180),
            ("saturation", p.saturation, -100, 100),
            ("lightness", p.lightness, -100, 100),
            ("alpha", p.alpha, 0, 100),
        ] {
            if !(min..=max).contains(&v) {
                return Err(NodeError::new(format!(
                    "`{name}` must be between {min} and {max}, got {v}"
                )));
            }
        }
        Ok(p)
    }
}

impl Node for Hsl {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "hsl",
            inputs: vec![PortSpec::new("layer", ValueType::Layer)],
            outputs: vec![PortSpec::new("out", ValueType::Layer)],
            params: vec![
                ParamSpec::int("hue", -180, 180, "30")
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
                    .doc("opacity, in % of the original's"),
            ],
            gui: false,
        }
    }

    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        Params::parse(params).map(|_| ())
    }

    fn eval(&self, inputs: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let p = Params::parse(params)?;
        let (h, s, l, a) = (
            p.hue as f64,
            p.saturation as f64,
            p.lightness as f64,
            p.alpha as f64,
        );
        let layer = effect::map_pixels(inputs, |c| adjust(c, h, s, l, a))?;
        Ok(Outputs::out(Value::Layer(layer)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::test_support::check_layer_handling;
    use crate::test_util::{image, params, run};

    fn pixel(rgba: [u8; 4], p: &str) -> [u8; 4] {
        match run(
            &Hsl,
            vec![(
                "layer",
                vec![Value::Layer(conduit_core::Layer::new(image(1, 1, rgba)))],
            )],
            &params(p),
        ) {
            Ok(Value::Layer(l)) => l.image.get_pixel(0, 0).0,
            other => panic!("expected layer, got {other:?}"),
        }
    }

    #[test]
    fn adjusts_every_pixel() {
        assert_eq!(pixel([255, 0, 0, 255], "hue = 180"), [0, 255, 255, 255]);
        assert_eq!(
            pixel([255, 0, 0, 255], "saturation = -100"),
            [128, 128, 128, 255]
        );
        assert_eq!(pixel([255, 0, 0, 200], "alpha = 50"), [255, 0, 0, 100]);
        assert_eq!(pixel([12, 34, 56, 78], ""), [12, 34, 56, 78]);
    }

    #[test]
    fn handles_layers() {
        check_layer_handling(&Hsl, "hue = 90\nlightness = 10");
    }

    #[test]
    fn rejects_out_of_range() {
        assert!(Hsl.check_params(&params("hue = 181")).is_err());
        assert!(Hsl.check_params(&params("alpha = 101")).is_err());
        assert!(Hsl.check_params(&params("color = \"red\"")).is_err());
    }
}
