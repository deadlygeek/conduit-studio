use std::collections::HashMap;

use conduit_core::{
    EvalCtx, Inputs, Node, NodeError, NodeResult, NodeSpec, Outputs, ParamSpec, PortSpec, Rgba,
    Value, ValueType, parse_color, parse_params,
};
use serde::Deserialize;

/// A named set of colors. Each color is an output of its own: `brand.accent`.
pub struct Palette;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    colors: toml::Table,
    #[serde(default, rename = "source")]
    _source: Option<String>,
}

/// The colors in file order.
fn colors(params: &toml::Table) -> NodeResult<Vec<(String, Rgba<u8>)>> {
    let p: Params = parse_params(params)?;
    p.colors
        .into_iter()
        .map(|(name, v)| {
            let color = v
                .as_str()
                .ok_or_else(|| format!("expected a color like \"#264653\", got {v}"))
                .and_then(parse_color)
                .map_err(|e| NodeError::new(format!("colors.{name}: {e}")))?;
            Ok((name, color))
        })
        .collect()
}

impl Node for Palette {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "palette",
            inputs: vec![],
            outputs: vec![PortSpec::new("colors", ValueType::Color).entries()],
            params: vec![
                ParamSpec::colors(
                    "colors",
                    r##"{ c1 = "#264653", c2 = "#2a9d8f", c3 = "#e9c46a", c4 = "#f4a261", c5 = "#e76f51" }"##,
                )
                .doc("name = color; each one is an output"),
                ParamSpec::text("source", "\"https://coolors.co/…\"")
                    .optional()
                    .doc("where the colors came from"),
            ],
            gui: false,
        }
    }

    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        colors(params).map(|_| ())
    }

    fn eval(&self, _: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let values: HashMap<String, Value> = colors(params)?
            .into_iter()
            .map(|(name, c)| (name, Value::Color(c)))
            .collect();
        Ok(Outputs { values })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::params;

    #[test]
    fn each_color_is_an_output() {
        let p = params("[colors]\nprimary = \"#264653\"\naccent = \"red\"");
        let mut ctx = conduit_core::EvalCtx {
            node_id: "brand",
            gui: &mut conduit_core::NullHost,
        };
        let out = Palette.eval(&Inputs::default(), &p, &mut ctx).unwrap();
        assert!(matches!(out.values["accent"], Value::Color(c) if c.0 == [255, 0, 0, 255]));
        assert_eq!(out.values.len(), 2);
        assert!(
            Palette
                .check_params(&params("[colors]\nx = \"nope\""))
                .is_err()
        );
    }
}
