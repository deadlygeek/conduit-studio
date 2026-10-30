use conduit_core::{
    EvalCtx, Inputs, Layer, Node, NodeResult, NodeSpec, Outputs, ParamSpec, PortSpec, Value,
    ValueType, parse_params,
};
use serde::Deserialize;

/// Places an image at an offset, producing a layer for compositing.
pub struct Position;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    #[serde(default)]
    x: i64,
    #[serde(default)]
    y: i64,
}

impl Node for Position {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "position",
            inputs: vec![PortSpec::new("image", ValueType::Image)],
            outputs: vec![PortSpec::new("out", ValueType::Layer)],
            params: vec![
                ParamSpec::int("x", i64::MIN, i64::MAX, "0")
                    .default("0")
                    .doc("left edge, in pixels"),
                ParamSpec::int("y", i64::MIN, i64::MAX, "0")
                    .default("0")
                    .doc("top edge, in pixels"),
            ],
            gui: false,
        }
    }

    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        parse_params::<Params>(params).map(|_| ())
    }

    fn eval(&self, inputs: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let p: Params = parse_params(params)?;
        let image = inputs.image("image")?.clone();
        Ok(Outputs::out(Value::Layer(Layer {
            image,
            x: p.x,
            y: p.y,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{image, params, run};

    #[test]
    fn wraps_image_at_offset() {
        let inputs = vec![("image", vec![Value::Image(image(3, 3, [0, 0, 0, 255]))])];
        let out = run(&Position, inputs, &params("x = -5\ny = 12")).unwrap();
        let Value::Layer(layer) = out else {
            panic!("expected layer")
        };
        assert_eq!((layer.x, layer.y, layer.image.width()), (-5, 12, 3));
    }

    #[test]
    fn defaults_to_origin_and_rejects_unknown_params() {
        let inputs = vec![("image", vec![Value::Image(image(1, 1, [0; 4]))])];
        let Value::Layer(layer) = run(&Position, inputs, &params("")).unwrap() else {
            panic!("expected layer")
        };
        assert_eq!((layer.x, layer.y), (0, 0));
        assert!(Position.check_params(&params("z = 1")).is_err());
    }
}
