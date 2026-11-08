//! Turning layers into pixels: drawing a layer onto a canvas (resampling it once with its
//! final transform) and realizing a layer onto its own bounding box.

use std::sync::Arc;

use image::{Rgba, RgbaImage};

use crate::affine::Filter;
use crate::value::Layer;

/// The pixel rectangle `[x0, x1) × [y0, y1)` a layer can touch in its parent, including
/// the soft edge its filter adds. `None` if it draws nothing (empty image, a transform
/// that flattens it, or one that isn't finite).
pub fn bounds(layer: &Layer) -> Option<(i64, i64, i64, i64)> {
    let (w, h) = (layer.image.width(), layer.image.height());
    if w == 0 || h == 0 {
        return None;
    }
    if let Some((x, y)) = layer.transform.whole_pixel_offset() {
        return Some((x, y, x + i64::from(w), y + i64::from(h)));
    }
    let m = &layer.transform;
    if !m.is_finite() || m.invert().is_none() {
        return None;
    }
    // Interpolating filters blend in transparent pixels past the edge.
    let pad = match layer.filter {
        Filter::Nearest => 0.0,
        Filter::Bilinear => 1.0,
        Filter::Bicubic => 2.0,
    };
    let (w, h) = (f64::from(w) + pad, f64::from(h) + pad);
    let corners = [(-pad, -pad), (w, -pad), (-pad, h), (w, h)].map(|(x, y)| m.apply(x, y));
    // Snap near-whole coordinates so rounding noise doesn't add an empty row.
    let snap = |v: f64| {
        if (v - v.round()).abs() < 1e-6 {
            v.round()
        } else {
            v
        }
    };
    let fold = |pick: fn(f64, f64) -> f64, axis: fn(&(f64, f64)) -> f64| {
        corners.iter().map(axis).map(snap).reduce(pick).unwrap()
    };
    let (x0, x1) = (fold(f64::min, |p| p.0), fold(f64::max, |p| p.0));
    let (y0, y1) = (fold(f64::min, |p| p.1), fold(f64::max, |p| p.1));
    // `as` saturates, so absurd scales stay representable and get clipped by the caller.
    Some((
        x0.floor() as i64,
        y0.floor() as i64,
        x1.ceil() as i64,
        y1.ceil() as i64,
    ))
}

/// Draws `layer` over `canvas` (alpha-over), clipped to the canvas. `origin` is where the
/// canvas's top-left pixel sits in the layer's parent space; (0, 0) when drawing onto the
/// parent itself.
pub fn draw(canvas: &mut RgbaImage, layer: &Layer, origin: (i64, i64)) {
    let Some((x0, y0, x1, y1)) = bounds(layer) else {
        return;
    };
    let (cw, ch) = (i64::from(canvas.width()), i64::from(canvas.height()));
    let (ox, oy) = origin;
    // Canvas pixel range to visit.
    let (px0, py0) = ((x0 - ox).max(0), (y0 - oy).max(0));
    let (px1, py1) = (x1.saturating_sub(ox).min(cw), y1.saturating_sub(oy).min(ch));
    if px0 >= px1 || py0 >= py1 {
        return;
    }

    if let Some((lx, ly)) = layer.transform.whole_pixel_offset() {
        // Exact copy: no resampling.
        for py in py0..py1 {
            for px in px0..px1 {
                let src = layer
                    .image
                    .get_pixel((px + ox - lx) as u32, (py + oy - ly) as u32);
                let dst = canvas.get_pixel_mut(px as u32, py as u32);
                *dst = blend_over(*src, *dst);
            }
        }
        return;
    }

    let inv = layer.transform.invert().expect("bounds checked it");
    for py in py0..py1 {
        for px in px0..px1 {
            // Map this pixel's center back into the layer's image.
            let (u, v) = inv.apply((px + ox) as f64 + 0.5, (py + oy) as f64 + 0.5);
            let src = sample(&layer.image, u - 0.5, v - 0.5, layer.filter);
            if src[3] == 0 {
                continue;
            }
            let dst = canvas.get_pixel_mut(px as u32, py as u32);
            *dst = blend_over(src, *dst);
        }
    }
}

/// Renders a layer onto a transparent image covering just its [`bounds`], so pixel
/// effects can work on it. The result only moves by whole pixels. Layers that already
/// do are returned unchanged.
pub fn realize(layer: &Layer) -> Layer {
    if layer.transform.whole_pixel_offset().is_some() {
        return layer.clone();
    }
    let Some((x0, y0, x1, y1)) = bounds(layer) else {
        return Layer {
            filter: layer.filter,
            ..Layer::new(Arc::new(RgbaImage::new(0, 0)))
        };
    };
    let mut image = RgbaImage::new((x1 - x0) as u32, (y1 - y0) as u32);
    draw(&mut image, layer, (x0, y0));
    Layer {
        filter: layer.filter,
        ..Layer::at(Arc::new(image), x0, y0)
    }
}

/// The color at point (x, y) of `image`, where integer coordinates are pixel centers.
/// Outside the image is transparent.
fn sample(image: &RgbaImage, x: f64, y: f64, filter: Filter) -> Rgba<u8> {
    match filter {
        Filter::Nearest => {
            // The pixel whose square contains the point.
            let p = texel(image, (x + 0.5).floor() as i64, (y + 0.5).floor() as i64);
            unpremultiply(p)
        }
        Filter::Bilinear => {
            let (fx, fy) = (x.floor(), y.floor());
            let (tx, ty) = ((x - fx) as f32, (y - fy) as f32);
            let wx = [1.0 - tx, tx];
            let wy = [1.0 - ty, ty];
            unpremultiply(weighted(image, fx as i64, fy as i64, &wx, &wy))
        }
        Filter::Bicubic => {
            let (fx, fy) = (x.floor(), y.floor());
            let wx = catmull_rom((x - fx) as f32);
            let wy = catmull_rom((y - fy) as f32);
            unpremultiply(weighted(image, fx as i64 - 1, fy as i64 - 1, &wx, &wy))
        }
    }
}

/// Weighted sum of premultiplied texels in the grid starting at (x0, y0).
fn weighted(image: &RgbaImage, x0: i64, y0: i64, wx: &[f32], wy: &[f32]) -> [f32; 4] {
    let mut acc = [0.0f32; 4];
    for (j, wy) in wy.iter().enumerate() {
        for (i, wx) in wx.iter().enumerate() {
            let t = texel(image, x0 + i as i64, y0 + j as i64);
            let w = wx * wy;
            for k in 0..4 {
                acc[k] += t[k] * w;
            }
        }
    }
    acc
}

/// Catmull-Rom weights for the four texels around a point `t` (0..1) past the second.
fn catmull_rom(t: f32) -> [f32; 4] {
    [
        ((-t + 2.0) * t - 1.0) * t / 2.0,
        ((3.0 * t - 5.0) * t * t + 2.0) / 2.0,
        ((-3.0 * t + 4.0) * t + 1.0) * t / 2.0,
        (t - 1.0) * t * t / 2.0,
    ]
}

/// A pixel premultiplied by its alpha (0..255 scale); transparent outside the image.
fn texel(image: &RgbaImage, x: i64, y: i64) -> [f32; 4] {
    if x < 0 || y < 0 || x >= i64::from(image.width()) || y >= i64::from(image.height()) {
        return [0.0; 4];
    }
    let p = image.get_pixel(x as u32, y as u32).0.map(f32::from);
    let a = p[3] / 255.0;
    [p[0] * a, p[1] * a, p[2] * a, p[3]]
}

fn unpremultiply(p: [f32; 4]) -> Rgba<u8> {
    // Bicubic can overshoot; keep alpha in range and colors within it.
    let a = p[3].clamp(0.0, 255.0);
    if a < 0.5 {
        return Rgba([0, 0, 0, 0]);
    }
    let channel = |c: f32| (c.clamp(0.0, a) * 255.0 / a).round() as u8;
    Rgba([channel(p[0]), channel(p[1]), channel(p[2]), a.round() as u8])
}

/// `image` with every pixel's color mixed toward `color` by `amount` (0–1), alpha kept so
/// shapes stay as they were.
pub fn tint_toward(image: &RgbaImage, color: [u8; 3], amount: f32) -> RgbaImage {
    let mut out = image.clone();
    for p in out.pixels_mut() {
        for i in 0..3 {
            let c = f32::from(p[i]);
            p[i] = (c + (f32::from(color[i]) - c) * amount).round() as u8;
        }
    }
    out
}

/// Alpha-over of `src` onto `dst`. Written by hand because `image::imageops::overlay`
/// rounds an opaque result down to alpha 254.
pub fn blend_over(src: Rgba<u8>, dst: Rgba<u8>) -> Rgba<u8> {
    match src[3] {
        255 => return src,
        0 => return dst,
        _ => {}
    }
    let sa = f32::from(src[3]) / 255.0;
    let da = f32::from(dst[3]) / 255.0;
    let out_a = sa + da * (1.0 - sa);
    let channel = |i: usize| {
        let c = (f32::from(src[i]) * sa + f32::from(dst[i]) * da * (1.0 - sa)) / out_a;
        c.round() as u8
    };
    Rgba([
        channel(0),
        channel(1),
        channel(2),
        (out_a * 255.0).round() as u8,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::affine::Affine;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const CLEAR: [u8; 4] = [0, 0, 0, 0];

    fn layer(w: u32, h: u32, transform: Affine, filter: Filter) -> Layer {
        Layer {
            image: Arc::new(RgbaImage::from_pixel(w, h, Rgba(RED))),
            transform,
            filter,
        }
    }

    #[test]
    fn whole_pixel_moves_copy_exactly() {
        let l = layer(2, 2, Affine::translate(1.0, 1.0), Filter::Bicubic);
        let mut canvas = RgbaImage::new(4, 4);
        draw(&mut canvas, &l, (0, 0));
        assert_eq!(canvas.get_pixel(1, 1).0, RED);
        assert_eq!(canvas.get_pixel(2, 2).0, RED);
        assert_eq!(canvas.get_pixel(3, 3).0, CLEAR);
        assert_eq!(bounds(&l), Some((1, 1, 3, 3)));
    }

    #[test]
    fn quarter_turn_with_nearest_is_exact() {
        // A 4×2 bar turned 90° clockwise about the origin covers x -2..0, y 0..4.
        let l = layer(4, 2, Affine::rotate(90.0), Filter::Nearest);
        assert_eq!(bounds(&l), Some((-2, 0, 0, 4)));
        let r = realize(&l);
        assert_eq!(r.image.dimensions(), (2, 4));
        assert_eq!(r.transform.whole_pixel_offset(), Some((-2, 0)));
        assert!(r.image.pixels().all(|p| p.0 == RED));
    }

    #[test]
    fn interpolated_edges_are_soft_but_inside_is_solid() {
        let l = layer(4, 4, Affine::translate(0.5, 0.0), Filter::Bilinear);
        let r = realize(&l);
        // Half a pixel to the right: the first and last columns are half covered.
        assert_eq!(r.transform.whole_pixel_offset(), Some((-1, -1)));
        let row = |x| r.image.get_pixel(x, 1).0;
        assert_eq!(row(1), [255, 0, 0, 128]);
        assert_eq!(row(2), RED);
        assert_eq!(row(5), [255, 0, 0, 128]);
        // Premultiplied sampling: no dark fringe on the soft edge.
        assert_eq!(row(1)[0], 255);
    }

    #[test]
    fn scale_up_is_solid_inside_and_soft_at_the_edge() {
        for filter in [Filter::Bilinear, Filter::Bicubic] {
            let l = layer(2, 2, Affine::scale(3.0, 3.0), filter);
            let mut canvas = RgbaImage::new(6, 6);
            draw(&mut canvas, &l, (0, 0));
            for x in 1..5 {
                assert_eq!(canvas.get_pixel(x, 2).0, RED, "{filter} x={x}");
            }
            let edge = canvas.get_pixel(0, 2).0;
            assert!(edge[3] > 0 && edge[3] < 255, "{filter}: {edge:?}");
        }
        // Nearest keeps hard edges.
        let l = layer(2, 2, Affine::scale(3.0, 3.0), Filter::Nearest);
        let mut canvas = RgbaImage::new(6, 6);
        draw(&mut canvas, &l, (0, 0));
        assert!(canvas.pixels().all(|p| p.0 == RED));
    }

    #[test]
    fn flattened_layers_draw_nothing() {
        let l = layer(2, 2, Affine::scale(0.0, 1.0), Filter::Bilinear);
        assert_eq!(bounds(&l), None);
        assert_eq!(realize(&l).image.dimensions(), (0, 0));
    }

    #[test]
    fn layers_far_off_canvas_are_skipped() {
        let l = layer(2, 2, Affine::translate(1e12, 0.5), Filter::Bilinear);
        let mut canvas = RgbaImage::new(4, 4);
        draw(&mut canvas, &l, (0, 0));
        assert!(canvas.pixels().all(|p| p.0 == CLEAR));
    }
}
