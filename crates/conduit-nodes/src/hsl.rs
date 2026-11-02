use conduit_core::{
    EvalCtx, Inputs, Node, NodeResult, NodeSpec, Outputs, ParamSpec, PortSpec, Value, ValueType,
    adjust,
};
use serde::Deserialize;

use crate::effect;

/// Hue / saturation / lightness: the `color` node's adjustments, applied to every pixel
/// of a layer.
pub struct Hsl;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    hue: i64,
    saturation: i64,
    lightness: i64,
    alpha: i64,
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
        self.spec().parse_params::<Params>(params).map(|_| ())
    }

    fn eval(&self, inputs: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let p: Params = self.spec().parse_params(params)?;
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
    use crate::test_util::params;

    fn pixel(rgba: [u8; 4], p: &str) -> [u8; 4] {
        crate::effect::test_support::pixel(&Hsl, rgba, p)
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
