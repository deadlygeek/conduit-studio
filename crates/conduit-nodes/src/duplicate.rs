use std::collections::HashMap;

use conduit_core::{
    EvalCtx, Inputs, Node, NodeResult, NodeSpec, Outputs, ParamSpec, PortSpec, Value, ValueType,
};
use serde::Deserialize;

/// Splits a layer into `count` identical outputs (`out1`, `out2`, …), a visible branch
/// point where each copy can go on to its own transform. No pixels are copied.
pub struct Duplicate;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    count: i64,
}

impl Node for Duplicate {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "duplicate",
            inputs: vec![PortSpec::new("layer", ValueType::Layer)],
            outputs: vec![PortSpec::new("out", ValueType::Layer).repeated(&["count"])],
            params: vec![
                ParamSpec::int("count", 1, 32, "2")
                    .default("2")
                    .fixed()
                    .doc("number of copies; each is an output"),
            ],
            gui: false,
        }
    }

    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        self.spec().parse_params::<Params>(params).map(|_| ())
    }

    fn eval(&self, inputs: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let p: Params = self.spec().parse_params(params)?;
        let layer = inputs.layer("layer")?;
        let values: HashMap<String, Value> = (1..=p.count)
            .map(|i| (format!("out{i}"), Value::Layer(layer.clone())))
            .collect();
        Ok(Outputs { values })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{image, params};
    use conduit_core::{Layer, NullHost};
    use std::sync::Arc;

    fn outputs(params: &toml::Table) -> NodeResult<Outputs> {
        let layer = Layer::at(image(2, 2, [1, 2, 3, 4]), 5, 6);
        let inputs = Inputs::new(HashMap::from([(
            "layer".to_string(),
            vec![Value::Layer(layer)],
        )]));
        let mut ctx = EvalCtx {
            node_id: "dup",
            root: std::path::Path::new(""),
            gui: &mut NullHost,
        };
        Duplicate.eval(&inputs, params, &mut ctx)
    }

    #[test]
    fn each_copy_is_an_output() {
        let out = outputs(&params("count = 3")).unwrap();
        let mut names: Vec<_> = out.values.keys().cloned().collect();
        names.sort();
        assert_eq!(names, ["out1", "out2", "out3"]);
        let Value::Layer(first) = &out.values["out1"] else {
            panic!("expected a layer")
        };
        for v in out.values.values() {
            let Value::Layer(l) = v else {
                panic!("expected a layer")
            };
            assert!(Arc::ptr_eq(&l.image, &first.image), "pixels are shared");
            assert_eq!(l.transform, first.transform);
        }
    }

    #[test]
    fn defaults_to_two() {
        assert_eq!(outputs(&toml::Table::new()).unwrap().values.len(), 2);
        let spec = Duplicate.spec().with_param_inputs();
        assert_eq!(spec.outputs_for(&toml::Table::new()).len(), 2);
        assert!(spec.input("count").is_none(), "count isn't linkable");
    }

    #[test]
    fn rejects_out_of_range_counts() {
        assert!(Duplicate.check_params(&params("count = 0")).is_err());
        assert!(Duplicate.check_params(&params("count = 33")).is_err());
    }
}
