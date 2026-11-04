use std::collections::HashMap;

use conduit_core::{
    Affine, DetachPlan, DetachedNode, DetachedSource, EvalCtx, Inputs, Layer, MAX_REPEAT, Node,
    NodeError, NodeResult, NodeSpec, Outputs, ParamSpec, PortSpec, Value, ValueType, float_value,
};
use serde::Deserialize;

/// Repeats a layer in a row, column or grid, like Blender's Array modifier. Each copy is
/// its own output (`out1`, `out2`, … across each row, then down), already moved into
/// place, so any one can go on to its own tint or transform. No pixels are copied.
pub struct Array;

const MAX_SIDE: i64 = 64;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    columns: i64,
    rows: i64,
    gap_x: f64,
    gap_y: f64,
    rotate_step: f64,
    scale_step: f64,
}

impl Params {
    fn parse(params: &toml::Table) -> NodeResult<Self> {
        let p: Params = Array.spec().parse_params(params)?;
        if p.columns * p.rows > MAX_REPEAT as i64 {
            return Err(NodeError::new(format!(
                "columns × rows must be at most {MAX_REPEAT}, got {}",
                p.columns * p.rows
            )));
        }
        for (name, v) in [
            ("gap_x", p.gap_x),
            ("gap_y", p.gap_y),
            ("rotate_step", p.rotate_step),
            ("scale_step", p.scale_step),
        ] {
            if !v.is_finite() {
                return Err(NodeError::new(format!("`{name}` must be a finite number")));
            }
        }
        Ok(p)
    }

    /// Copy `i`'s rotation and scale, with no move.
    fn turn(&self, i: i64) -> Affine {
        let step = i as f64;
        Affine::rotate(self.rotate_step * step)
            * Affine::scale(self.scale_step.powf(step), self.scale_step.powf(step))
    }

    /// The `transform` params that place copy `i` as [`Params::matrix`] does. `transform`
    /// pivots about the image's centre, the array about the copy's cell centre, so the move
    /// makes up the difference.
    fn transform_params(&self, layer: &Layer, i: i64) -> toml::Table {
        let (w, h) = layer.image.dimensions();
        let (px, py) = layer
            .transform
            .apply(f64::from(w) / 2.0, f64::from(h) / 2.0);
        // Where the transform's pivot ends up under the array's matrix, minus where it was.
        let (qx, qy) = self.matrix(layer, i).apply(px, py);
        let round = |v: f64| float_value((v * 1e6).round() / 1e6);
        let mut params = toml::Table::new();
        params.insert("x".into(), round(qx - px));
        params.insert("y".into(), round(qy - py));
        let rotate = self.rotate_step * i as f64;
        if rotate != 0.0 {
            params.insert("rotate".into(), round(rotate));
        }
        let scale = self.scale_step.powf(i as f64);
        if scale != 1.0 {
            params.insert("scale".into(), round(scale));
        }
        params
    }

    /// The transform that puts copy `i` (counting from 0, across then down) in place, on
    /// top of the incoming layer's own: rotated and scaled about the layer's centre by
    /// `i` steps, then moved one cell per column and row. A cell is the layer's bounding
    /// box in its parent plus the gap.
    fn matrix(&self, layer: &Layer, i: i64) -> Affine {
        let (x0, y0, x1, y1) = bounds(layer);
        let (col, row) = (i % self.columns, i / self.columns);
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        Affine::translate(
            col as f64 * (x1 - x0 + self.gap_x),
            row as f64 * (y1 - y0 + self.gap_y),
        ) * Affine::translate(cx, cy)
            * self.turn(i)
            * Affine::translate(-cx, -cy)
    }
}

/// The layer's bounding box in its parent: (left, top, right, bottom).
fn bounds(layer: &Layer) -> (f64, f64, f64, f64) {
    let (w, h) = layer.image.dimensions();
    let (w, h) = (f64::from(w), f64::from(h));
    let corners =
        [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)].map(|(x, y)| layer.transform.apply(x, y));
    corners.iter().fold(
        (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ),
        |(l, t, r, b), &(x, y)| (l.min(x), t.min(y), r.max(x), b.max(y)),
    )
}

impl Node for Array {
    fn spec(&self) -> NodeSpec {
        let any = |name, example| ParamSpec::float(name, f64::NEG_INFINITY, f64::INFINITY, example);
        NodeSpec {
            kind: "array",
            inputs: vec![PortSpec::new("layer", ValueType::Layer)],
            outputs: vec![PortSpec::new("out", ValueType::Layer).repeated(&["columns", "rows"])],
            params: vec![
                ParamSpec::int("columns", 1, MAX_SIDE, "3")
                    .default("3")
                    .fixed()
                    .doc("copies across; outputs are columns × rows"),
                ParamSpec::int("rows", 1, MAX_SIDE, "1")
                    .default("1")
                    .fixed()
                    .doc("copies down"),
                any("gap_x", "0")
                    .default("0")
                    .doc("space between columns, in parent pixels (negative overlaps)"),
                any("gap_y", "0")
                    .default("0")
                    .doc("space between rows, in parent pixels (negative overlaps)"),
                any("rotate_step", "0")
                    .default("0")
                    .doc("each copy turns this many more degrees than the one before"),
                ParamSpec::float("scale_step", 0.01, 100.0, "1")
                    .default("1")
                    .doc("each copy is this many times the size of the one before"),
            ],
            gui: false,
        }
    }

    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        Params::parse(params).map(|_| ())
    }

    fn eval(&self, inputs: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let p = Params::parse(params)?;
        let layer = inputs.layer("layer")?;
        let values: HashMap<String, Value> = (0..p.columns * p.rows)
            .map(|i| {
                let copy = Layer {
                    image: layer.image.clone(),
                    transform: p.matrix(layer, i) * layer.transform,
                    filter: layer.filter,
                };
                (format!("out{}", i + 1), Value::Layer(copy))
            })
            .collect();
        Ok(Outputs { values })
    }

    fn can_detach(&self) -> bool {
        true
    }

    /// Each copy becomes a `transform` of whatever fed the array.
    fn detach(
        &self,
        inputs: &Inputs,
        params: &toml::Table,
        ctx: &mut EvalCtx,
    ) -> Option<NodeResult<DetachPlan>> {
        let plan = || {
            let p = Params::parse(params)?;
            let layer = inputs.layer("layer")?;
            let (mut nodes, mut outputs) = (Vec::new(), Vec::new());
            for i in 0..p.columns * p.rows {
                let out = format!("out{}", i + 1);
                nodes.push(DetachedNode {
                    name: out.clone(),
                    comment: format!(
                        "Copy {} (column {}, row {}) of {}'s array, detached",
                        i + 1,
                        i % p.columns + 1,
                        i / p.columns + 1,
                        ctx.node_id
                    ),
                    kind: "transform",
                    inputs: vec![("layer".into(), DetachedSource::Input("layer".into()))],
                    params: p.transform_params(layer, i),
                });
                outputs.push((out.clone(), out));
            }
            Ok(DetachPlan { nodes, outputs })
        };
        Some(plan())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{image, params};
    use conduit_core::NullHost;
    use std::sync::Arc;

    fn outputs(layer: Layer, params: &toml::Table) -> NodeResult<Outputs> {
        let inputs = Inputs::new(HashMap::from([(
            "layer".to_string(),
            vec![Value::Layer(layer)],
        )]));
        let mut ctx = EvalCtx {
            node_id: "arr",
            root: std::path::Path::new(""),
            gui: &mut NullHost,
        };
        Array.eval(&inputs, params, &mut ctx)
    }

    fn layer(out: &Outputs, name: &str) -> Layer {
        match &out.values[name] {
            Value::Layer(l) => l.clone(),
            _ => panic!("expected a layer"),
        }
    }

    fn offset(out: &Outputs, name: &str) -> (i64, i64) {
        layer(out, name)
            .transform
            .whole_pixel_offset()
            .expect("a whole-pixel move")
    }

    #[test]
    fn a_row_by_default() {
        let tile = Layer::at(image(10, 4, [1, 2, 3, 4]), 5, 6);
        let out = outputs(tile.clone(), &toml::Table::new()).unwrap();
        assert_eq!(out.values.len(), 3);
        assert_eq!(offset(&out, "out1"), (5, 6));
        assert_eq!(offset(&out, "out2"), (15, 6));
        assert_eq!(offset(&out, "out3"), (25, 6));
        for v in out.values.values() {
            let Value::Layer(l) = v else { unreachable!() };
            assert!(Arc::ptr_eq(&l.image, &tile.image), "pixels are shared");
        }
    }

    #[test]
    fn a_grid_counts_across_then_down_with_gaps() {
        let tile = Layer::at(image(10, 4, [1, 2, 3, 4]), 0, 0);
        let out = outputs(tile, &params("columns = 2\nrows = 3\ngap_x = 5\ngap_y = 1")).unwrap();
        assert_eq!(out.values.len(), 6);
        assert_eq!(offset(&out, "out1"), (0, 0));
        assert_eq!(offset(&out, "out2"), (15, 0));
        assert_eq!(offset(&out, "out3"), (0, 5));
        assert_eq!(offset(&out, "out6"), (15, 10));
        let spec = Array.spec().with_param_inputs();
        assert_eq!(
            spec.outputs_for(&params("columns = 2\nrows = 3")).len(),
            6,
            "the declared outputs match"
        );
        assert!(spec.input("columns").is_none() && spec.input("gap_x").is_some());
    }

    #[test]
    fn steps_accumulate_about_each_copys_centre() {
        let tile = Layer::at(image(10, 10, [1, 2, 3, 4]), 0, 0);
        let out = outputs(tile, &params("rotate_step = 90\nscale_step = 0.5")).unwrap();
        // Copy 3 is turned 180° and quartered about the centre of its cell (25, 5).
        let m = layer(&out, "out3").transform;
        let (x, y) = m.apply(0.0, 0.0);
        assert!(
            (x - 26.25).abs() < 1e-9 && (y - 6.25).abs() < 1e-9,
            "{x},{y}"
        );
        let (x, y) = m.apply(5.0, 5.0);
        assert!(
            (x - 25.0).abs() < 1e-9 && (y - 5.0).abs() < 1e-9,
            "centre stays put"
        );
    }

    #[test]
    fn rejects_bad_params() {
        assert!(Array.check_params(&params("columns = 0")).is_err());
        assert!(Array.check_params(&params("rows = 65")).is_err());
        assert!(Array.check_params(&params("scale_step = 0")).is_err());
        assert!(
            Array
                .check_params(&params("columns = 16\nrows = 16"))
                .is_ok()
        );
        assert!(
            Array
                .check_params(&params("columns = 17\nrows = 16"))
                .is_err()
        );
    }
}
