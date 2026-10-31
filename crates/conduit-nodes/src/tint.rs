use conduit_core::{
    Color, EvalCtx, Inputs, Node, NodeError, NodeResult, NodeSpec, Outputs, ParamSpec, PortSpec,
    Rgba, Value, ValueType, luminance, parse_params,
};
use serde::Deserialize;

use crate::effect;

/// After Effects' Tint: each pixel's lightness picks a color between `black` and `white`,
/// mixed with the original by `amount`. With the defaults this makes a layer grayscale;
/// linking `black` to a palette color recolors a dark logo.
pub struct Tint;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    #[serde(default = "black")]
    black: Color,
    #[serde(default = "white")]
    white: Color,
    #[serde(default = "full")]
    amount: i64,
}

fn black() -> Color {
    Color(Rgba([0, 0, 0, 255]))
}

fn white() -> Color {
    Color(Rgba([255, 255, 255, 255]))
}

fn full() -> i64 {
    100
}

impl Params {
    fn parse(params: &toml::Table) -> NodeResult<Self> {
        let p: Self = parse_params(params)?;
        if !(0..=100).contains(&p.amount) {
            return Err(NodeError::new(format!(
                "`amount` must be between 0 and 100, got {}",
                p.amount
            )));
        }
        Ok(p)
    }
}

fn lerp(a: u8, b: u8, t: f64) -> f64 {
    f64::from(a) + (f64::from(b) - f64::from(a)) * t
}

/// `c` tinted: its luminance mapped between `black` and `white`, then mixed with `c` by
/// `amount` (0–1). The mapped color's alpha scales `c`'s.
fn tint(c: Rgba<u8>, black: Rgba<u8>, white: Rgba<u8>, amount: f64) -> Rgba<u8> {
    let l = luminance(c);
    let mut out = [0u8; 4];
    for i in 0..3 {
        let mapped = lerp(black[i], white[i], l);
        out[i] = (f64::from(c[i]) + (mapped - f64::from(c[i])) * amount).round() as u8;
    }
    let mapped_alpha = lerp(black[3], white[3], l) / 255.0;
    let alpha_scale = 1.0 + (mapped_alpha - 1.0) * amount;
    out[3] = (f64::from(c[3]) * alpha_scale).round() as u8;
    Rgba(out)
}

impl Node for Tint {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "tint",
            inputs: vec![PortSpec::new("layer", ValueType::Layer)],
            outputs: vec![PortSpec::new("out", ValueType::Layer)],
            params: vec![
                ParamSpec::color("black", "\"black\"")
                    .default("\"black\"")
                    .doc("color that black (dark pixels) becomes"),
                ParamSpec::color("white", "\"white\"")
                    .default("\"white\"")
                    .doc("color that white (light pixels) becomes"),
                ParamSpec::int("amount", 0, 100, "100")
                    .default("100")
                    .doc("how much to tint, in %"),
            ],
            gui: false,
        }
    }

    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        Params::parse(params).map(|_| ())
    }

    fn eval(&self, inputs: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let p = Params::parse(params)?;
        let amount = p.amount as f64 / 100.0;
        let layer = effect::map_pixels(inputs, |c| tint(c, p.black.0, p.white.0, amount))?;
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
            &Tint,
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
    fn defaults_make_grayscale() {
        // Rec. 709: pure green is much lighter than pure blue.
        assert_eq!(pixel([0, 255, 0, 255], ""), [182, 182, 182, 255]);
        assert_eq!(pixel([0, 0, 255, 128], ""), [18, 18, 18, 128]);
        assert_eq!(pixel([255, 255, 255, 255], ""), [255, 255, 255, 255]);
    }

    #[test]
    fn maps_black_and_white() {
        assert_eq!(
            pixel([0, 0, 0, 255], "black = \"#e76f51\""),
            [0xe7, 0x6f, 0x51, 255]
        );
        assert_eq!(
            pixel([255, 255, 255, 200], "black = \"#e76f51\""),
            [255, 255, 255, 200]
        );
        assert_eq!(
            pixel([0, 0, 0, 255], "black = \"transparent\""),
            [0, 0, 0, 0],
            "the mapped alpha scales the pixel's"
        );
    }

    #[test]
    fn amount_mixes_with_the_original() {
        assert_eq!(pixel([200, 40, 40, 255], "amount = 0"), [200, 40, 40, 255]);
        assert_eq!(
            pixel([0, 0, 0, 255], "black = \"#ff0000\"\namount = 50"),
            [128, 0, 0, 255]
        );
    }

    #[test]
    fn handles_layers() {
        check_layer_handling(&Tint, "black = \"#264653\"\nwhite = \"#e9c46a\"");
    }

    #[test]
    fn rejects_out_of_range() {
        assert!(Tint.check_params(&params("amount = 101")).is_err());
        assert!(Tint.check_params(&params("black = \"nope\"")).is_err());
    }
}
