use std::sync::Arc;

use conduit_core::{
    EvalCtx, Inputs, Layer, Node, NodeError, NodeResult, NodeSpec, Outputs, PortSpec, RgbaImage,
    Value, ValueType, raster,
};

use crate::MAX_DIMENSION;

/// Draws several layers together into one layer that lands where they were, so a single
/// `transform` (or effect) after it acts on all of them as a unit, like a group in Figma
/// or Merge → Transform in Nuke. A transform of the group pivots on its bounding box's
/// centre by default.
pub struct Group;

impl Node for Group {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "group",
            inputs: vec![PortSpec::new("layers", ValueType::Layer).multi()],
            outputs: vec![PortSpec::new("out", ValueType::Layer)],
            params: vec![],
            gui: false,
        }
    }

    fn eval(&self, inputs: &Inputs, _: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let layers = inputs.layers("layers")?;
        Ok(Outputs::out(Value::Layer(group(&layers)?)))
    }
}

/// `layers` drawn in order onto their joint bounding box, placed where they were.
fn group(layers: &[&Layer]) -> NodeResult<Layer> {
    let joint = layers.iter().filter_map(|l| raster::bounds(l)).reduce(
        |(a0, b0, a1, b1), (x0, y0, x1, y1)| (a0.min(x0), b0.min(y0), a1.max(x1), b1.max(y1)),
    );
    let Some((x0, y0, x1, y1)) = joint else {
        return Ok(Layer::new(Arc::new(RgbaImage::new(0, 0))));
    };
    let (w, h) = (x1 - x0, y1 - y0);
    if w > i64::from(MAX_DIMENSION) || h > i64::from(MAX_DIMENSION) {
        return Err(NodeError::new(format!(
            "the group would be {w}x{h}, larger than the {MAX_DIMENSION}x{MAX_DIMENSION} limit"
        )));
    }
    let mut canvas = RgbaImage::new(w as u32, h as u32);
    for layer in layers {
        raster::draw(&mut canvas, layer, (x0, y0));
    }
    Ok(Layer::at(Arc::new(canvas), x0, y0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::composite::Composite;
    use crate::test_util::{image, run};
    use conduit_core::Affine;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const CLEAR: [u8; 4] = [0; 4];

    fn grouped(layers: Vec<Layer>) -> NodeResult<Layer> {
        let values = layers.into_iter().map(Value::Layer).collect();
        match run(&Group, vec![("layers", values)], &Default::default())? {
            Value::Layer(l) => Ok(l),
            _ => panic!("expected a layer"),
        }
    }

    #[test]
    fn covers_every_part_where_it_was() {
        let out = grouped(vec![
            Layer::at(image(2, 2, RED), 10, 5),
            Layer::at(image(3, 1, BLUE), 14, 8),
        ])
        .unwrap();
        assert_eq!(out.transform.whole_pixel_offset(), Some((10, 5)));
        assert_eq!(out.image.dimensions(), (7, 4));
        assert_eq!(out.image.get_pixel(0, 0).0, RED);
        assert_eq!(out.image.get_pixel(6, 3).0, BLUE);
        assert_eq!(out.image.get_pixel(3, 0).0, CLEAR, "gaps are transparent");
    }

    #[test]
    fn later_parts_draw_over_earlier_ones() {
        let out = grouped(vec![
            Layer::at(image(2, 2, RED), 0, 0),
            Layer::at(image(2, 2, BLUE), 1, 1),
        ])
        .unwrap();
        assert_eq!(out.image.get_pixel(1, 1).0, BLUE);
        assert_eq!(out.image.get_pixel(0, 0).0, RED);
    }

    #[test]
    fn nothing_to_draw_is_an_empty_layer() {
        let out = grouped(vec![Layer::new(image(0, 0, RED))]).unwrap();
        assert_eq!(out.image.dimensions(), (0, 0));
    }

    #[test]
    fn a_turned_part_lands_where_composite_draws_it() {
        let mut turned = Layer::new(image(8, 4, RED));
        turned.transform = Affine::translate(20.0, 10.0) * Affine::rotate(30.0);
        let parts = vec![turned, Layer::at(image(3, 3, BLUE), 2, 2)];

        let base = Value::Image(image(40, 30, CLEAR));
        let draw = |layers: Vec<Layer>| match run(
            &Composite,
            vec![
                ("base", vec![base.clone()]),
                ("layers", layers.into_iter().map(Value::Layer).collect()),
            ],
            &Default::default(),
        )
        .unwrap()
        {
            Value::Image(img) => img,
            _ => panic!("expected an image"),
        };
        let direct = draw(parts.clone());
        let via_group = draw(vec![grouped(parts).unwrap()]);
        assert_eq!(via_group.as_raw(), direct.as_raw());
    }

    #[test]
    fn rejects_huge_groups() {
        let err = grouped(vec![
            Layer::at(image(1, 1, RED), 0, 0),
            Layer::at(image(1, 1, RED), 20_000, 0),
        ])
        .err()
        .unwrap();
        assert!(err.0.contains("larger than the 16384x16384 limit"), "{err}");
    }
}
