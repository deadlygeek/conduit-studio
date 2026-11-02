//! Shared plumbing for pixel effects: nodes that take a layer, change its pixels and
//! output a layer that lands in the same place.

use std::sync::Arc;

use conduit_core::{Inputs, Layer, NodeError, NodeResult, Rgba, raster};

use crate::MAX_DIMENSION;

/// `layer` rendered onto its own bounding box (see [`raster::realize`]), refusing results
/// too large to allocate. Layers that only move by whole pixels come back unchanged.
pub fn realized(layer: &Layer) -> NodeResult<Layer> {
    if let Some((x0, y0, x1, y1)) = raster::bounds(layer) {
        let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
        if w > i64::from(MAX_DIMENSION) || h > i64::from(MAX_DIMENSION) {
            return Err(NodeError::new(format!(
                "the transformed layer would be {w}x{h}, larger than the {MAX_DIMENSION}x{MAX_DIMENSION} limit"
            )));
        }
    }
    Ok(raster::realize(layer))
}

/// Applies `f` to every pixel of the input layer. A rotated, scaled or skewed layer is
/// realized first, so the effect sees the pixels as they would be drawn.
pub fn map_pixels(inputs: &Inputs, f: impl Fn(Rgba<u8>) -> Rgba<u8>) -> NodeResult<Layer> {
    let mut layer = realized(inputs.layer("layer")?)?;
    let mut image = (*layer.image).clone();
    for px in image.pixels_mut() {
        *px = f(*px);
    }
    layer.image = Arc::new(image);
    Ok(layer)
}

#[cfg(test)]
pub mod test_support {
    //! Checks every effect should pass.

    use conduit_core::{Affine, Layer, Node, raster};

    use crate::test_util::{image, on_layer, params};

    /// The pixel `node` with params `p` makes of a 1×1 `rgba` image.
    pub fn pixel(node: &dyn Node, rgba: [u8; 4], p: &str) -> [u8; 4] {
        let out = on_layer(node, Layer::new(image(1, 1, rgba)), &params(p)).unwrap();
        out.image.get_pixel(0, 0).0
    }

    /// `node` with `p` keeps an untransformed image's size and place, and realizes a
    /// rotated layer before changing its pixels.
    pub fn check_layer_handling(node: &dyn Node, p: &str) {
        let p = params(p);
        let img = image(4, 2, [200, 40, 40, 255]);

        let out = on_layer(node, Layer::new(img.clone()), &p).unwrap();
        assert_eq!(out.image.dimensions(), (4, 2));
        assert_eq!(out.transform.whole_pixel_offset(), Some((0, 0)));

        let mut turned = Layer::new(img);
        turned.transform = Affine::translate(10.0, 5.0) * Affine::rotate(30.0);
        let out = on_layer(node, turned.clone(), &p).unwrap();
        assert!(out.transform.whole_pixel_offset().is_some());
        let expected = on_layer(node, raster::realize(&turned), &p).unwrap();
        assert_eq!(
            out.transform.whole_pixel_offset(),
            expected.transform.whole_pixel_offset()
        );
        assert_eq!(*out.image, *expected.image);
    }
}
