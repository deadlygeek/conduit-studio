use conduit_core::{
    EvalCtx, Inputs, Node, NodeResult, NodeSpec, Outputs, ParamSpec, PortSpec, Value, ValueType,
    parse_params,
};
use serde::Deserialize;

/// A whole number, for feeding other nodes' number params (sizes, positions, …).
pub struct Int;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    #[serde(default)]
    value: i64,
}

impl Node for Int {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "int",
            inputs: vec![],
            outputs: vec![PortSpec::new("out", ValueType::Number)],
            params: vec![
                ParamSpec::int("value", i64::MIN, i64::MAX, "0")
                    .default("0")
                    .doc("the number"),
            ],
            gui: false,
        }
    }

    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        parse_params::<Params>(params).map(|_| ())
    }

    fn eval(&self, _: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let p: Params = parse_params(params)?;
        Ok(Outputs::out(Value::Number(p.value as f64)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{params, run};

    #[test]
    fn outputs_its_value() {
        let out = run(&Int, vec![], &params("value = -42")).unwrap();
        assert!(matches!(out, Value::Number(n) if n == -42.0));
        let out = run(&Int, vec![], &params("")).unwrap();
        assert!(matches!(out, Value::Number(n) if n == 0.0));
        assert!(Int.check_params(&params("value = 1.5")).is_err());
    }
}
