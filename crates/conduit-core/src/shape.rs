//! Vector shapes: outlines that can be combined with boolean operations (union, subtract,
//! intersect, exclude) and turn into pixels only when drawn, the way Figma or Illustrator
//! shapes do.
//!
//! An outline is a set of closed polygons in parent pixel space, filled with the even-odd
//! rule. Curves (ovals, rounded corners) are flattened finely enough on creation that the
//! segments can't be seen, which lets booleans work on plain polygons.

use std::fmt;
use std::sync::Arc;

use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use image::{Rgba, RgbaImage};
use kurbo::{PathEl, Shape as _};

/// How far a flattened curve may stray from the true curve, in pixels.
const TOLERANCE: f64 = 0.05;

/// Longest side [`rasterize`] will allocate. Nodes reject larger shapes first, with a
/// message; this only guards against callers that don't.
const MAX_RASTER_SIDE: i64 = 1 << 15;

/// One closed polygon. The last point joins back to the first.
pub type Contour = Vec<[f64; 2]>;

/// How a shape is painted. Opacities multiply their color's own alpha.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub fill: Rgba<u8>,
    pub fill_opacity: f64,
    pub stroke: Rgba<u8>,
    pub stroke_opacity: f64,
    /// Centered on the outline. 0 draws no stroke.
    pub stroke_width: f64,
}

impl Default for Style {
    /// White fill, no stroke.
    fn default() -> Self {
        Self {
            fill: Rgba([255, 255, 255, 255]),
            fill_opacity: 1.0,
            stroke: Rgba([0, 0, 0, 255]),
            stroke_opacity: 1.0,
            stroke_width: 0.0,
        }
    }
}

/// Which boolean operation [`Shape::combine`] applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    /// Everything covered by either shape.
    Union,
    /// The first shape with the second cut out of it.
    Subtract,
    /// Only where both shapes overlap.
    Intersect,
    /// Everything covered by exactly one of the shapes.
    Exclude,
}

/// A vector outline plus how it is painted.
#[derive(Clone, PartialEq)]
pub struct Shape {
    pub contours: Arc<Vec<Contour>>,
    pub style: Style,
}

impl Shape {
    /// A `width` × `height` rectangle with its top-left at the origin and corners rounded
    /// by `radius` (clamped to half the shorter side).
    pub fn rectangle(width: f64, height: f64, radius: f64, style: Style) -> Self {
        let radius = radius.clamp(0.0, width.min(height) / 2.0);
        let rect = kurbo::RoundedRect::new(0.0, 0.0, width, height, radius);
        Self::from_path(rect.path_elements(TOLERANCE), style)
    }

    /// An oval filling the `width` × `height` box with its top-left at the origin.
    pub fn oval(width: f64, height: f64, style: Style) -> Self {
        let ellipse = kurbo::Ellipse::from_rect(kurbo::Rect::new(0.0, 0.0, width, height));
        Self::from_path(ellipse.path_elements(TOLERANCE), style)
    }

    fn from_path(path: impl IntoIterator<Item = PathEl>, style: Style) -> Self {
        let mut contours = Vec::new();
        let mut current: Contour = Vec::new();
        kurbo::flatten(path, TOLERANCE, |el| match el {
            PathEl::MoveTo(p) => {
                if current.len() >= 3 {
                    contours.push(std::mem::take(&mut current));
                }
                current.clear();
                current.push([p.x, p.y]);
            }
            // Skip repeats (a zero corner radius gives zero-length arcs).
            PathEl::LineTo(p) if current.last() != Some(&[p.x, p.y]) => {
                current.push([p.x, p.y]);
            }
            _ => {}
        });
        if current.len() >= 3 {
            contours.push(current);
        }
        for c in &mut contours {
            if c.len() > 1 && c.first() == c.last() {
                c.pop();
            }
        }
        contours.retain(|c| c.len() >= 3);
        Self {
            contours: Arc::new(contours),
            style,
        }
    }

    /// The shape rotated by `degrees` (clockwise, y-down) about the centre of its own
    /// `width` × `height` box at the origin, then moved by (`x`, `y`).
    pub fn placed(&self, x: f64, y: f64, degrees: f64, width: f64, height: f64) -> Self {
        let m = kurbo::Affine::translate((x, y))
            * kurbo::Affine::rotate_about(degrees.to_radians(), (width / 2.0, height / 2.0));
        let contours = self
            .contours
            .iter()
            .map(|c| {
                c.iter()
                    .map(|&[px, py]| {
                        let p = m * kurbo::Point::new(px, py);
                        [p.x, p.y]
                    })
                    .collect()
            })
            .collect();
        Self {
            contours: Arc::new(contours),
            style: self.style,
        }
    }

    /// `self` combined with `other` by `op`. The result keeps `self`'s style.
    pub fn combine(&self, other: &Shape, op: BooleanOp) -> Shape {
        let rule = match op {
            BooleanOp::Union => OverlayRule::Union,
            BooleanOp::Subtract => OverlayRule::Difference,
            BooleanOp::Intersect => OverlayRule::Intersect,
            BooleanOp::Exclude => OverlayRule::Xor,
        };
        let a: &Vec<Contour> = &self.contours;
        let b: &Vec<Contour> = &other.contours;
        let contours = if a.is_empty() || b.is_empty() {
            // The overlay library wants something on both sides.
            match op {
                BooleanOp::Intersect => Vec::new(),
                BooleanOp::Subtract => a.clone(),
                BooleanOp::Union | BooleanOp::Exclude if a.is_empty() => b.clone(),
                BooleanOp::Union | BooleanOp::Exclude => a.clone(),
            }
        } else {
            a.overlay_as::<i64>(b, rule, FillRule::EvenOdd)
                .into_iter()
                .flatten()
                .filter(|c| c.len() >= 3)
                .collect()
        };
        Shape {
            contours: Arc::new(contours),
            style: self.style,
        }
    }

    /// The outline's extent `(x0, y0, x1, y1)`, ignoring the stroke. `None` if empty.
    pub fn outline_bounds(&self) -> Option<(f64, f64, f64, f64)> {
        self.contours
            .iter()
            .flatten()
            .map(|&[x, y]| (x, y, x, y))
            .reduce(|(a0, b0, a1, b1), (x0, y0, x1, y1)| {
                (a0.min(x0), b0.min(y0), a1.max(x1), b1.max(y1))
            })
    }

    /// The pixel rectangle `[x0, x1) × [y0, y1)` the painted shape touches, stroke
    /// included. Tight: a shape whose edges lie on whole pixels gets no empty margin.
    /// `None` if it paints nothing.
    pub fn raster_bounds(&self) -> Option<(i64, i64, i64, i64)> {
        let (mut x0, mut y0, mut x1, mut y1) = self.outline_bounds()?;
        if ![x0, y0, x1, y1].iter().all(|v| v.is_finite()) {
            return None;
        }
        if self.style.stroke_width > 0.0 {
            // Measure the stroke (miters included) near the origin, where f32 is precise.
            let stroked = skia_path(self, x0, y0)?.stroke(&self.skia_stroke(), 1.0)?;
            let b = stroked.bounds();
            let (ox, oy) = (x0, y0);
            x0 = x0.min(ox + f64::from(b.left()));
            y0 = y0.min(oy + f64::from(b.top()));
            x1 = x1.max(ox + f64::from(b.right()));
            y1 = y1.max(oy + f64::from(b.bottom()));
        }
        // Snap near-whole coordinates so rounding noise doesn't add an empty row.
        let snap = |v: f64| {
            if (v - v.round()).abs() < 1e-4 {
                v.round()
            } else {
                v
            }
        };
        // `as` saturates, so absurd coordinates stay representable.
        Some((
            snap(x0).floor() as i64,
            snap(y0).floor() as i64,
            snap(x1).ceil() as i64,
            snap(y1).ceil() as i64,
        ))
    }

    fn skia_stroke(&self) -> tiny_skia::Stroke {
        tiny_skia::Stroke {
            width: self.style.stroke_width as f32,
            line_join: tiny_skia::LineJoin::Miter,
            ..Default::default()
        }
    }
}

impl fmt::Display for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let n = self.contours.len();
        let s = if n == 1 { "" } else { "s" };
        match self.outline_bounds() {
            Some((x0, y0, x1, y1)) => write!(
                f,
                "{n} contour{s}, {:.1}x{:.1} at {:.1},{:.1}",
                x1 - x0,
                y1 - y0,
                x0,
                y0
            ),
            None => f.write_str("empty"),
        }
    }
}

impl fmt::Debug for Shape {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Shape({self})")
    }
}

/// Paints `shape` onto a transparent image covering its [`Shape::raster_bounds`].
/// Returns the image and where its top-left pixel sits in the parent. An empty shape
/// gives a 0×0 image.
pub fn rasterize(shape: &Shape) -> (RgbaImage, i64, i64) {
    let empty = (RgbaImage::new(0, 0), 0, 0);
    let Some((x0, y0, x1, y1)) = shape.raster_bounds() else {
        return empty;
    };
    let (w, h) = (x1 - x0, y1 - y0);
    if w <= 0 || h <= 0 || w > MAX_RASTER_SIDE || h > MAX_RASTER_SIDE {
        return empty;
    }
    let Some(mut pixmap) = tiny_skia::Pixmap::new(w as u32, h as u32) else {
        return empty;
    };
    let Some(path) = skia_path(shape, x0 as f64, y0 as f64) else {
        return empty;
    };

    let style = &shape.style;
    let paint = |color: Rgba<u8>, opacity: f64| {
        let [r, g, b, a] = color.0;
        let alpha = (f64::from(a) * opacity.clamp(0.0, 1.0)).round() as u8;
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(r, g, b, alpha);
        paint.anti_alias = true;
        paint
    };
    let identity = tiny_skia::Transform::identity();
    pixmap.fill_path(
        &path,
        &paint(style.fill, style.fill_opacity),
        tiny_skia::FillRule::EvenOdd,
        identity,
        None,
    );
    if style.stroke_width > 0.0 {
        pixmap.stroke_path(
            &path,
            &paint(style.stroke, style.stroke_opacity),
            &shape.skia_stroke(),
            identity,
            None,
        );
    }

    // tiny-skia stores premultiplied alpha; images here are straight.
    let mut image = RgbaImage::new(w as u32, h as u32);
    for (dst, src) in image.pixels_mut().zip(pixmap.pixels()) {
        let c = src.demultiply();
        *dst = Rgba([c.red(), c.green(), c.blue(), c.alpha()]);
    }
    (image, x0, y0)
}

/// The shape's contours as a tiny-skia path, shifted so (`ox`, `oy`) is the origin.
fn skia_path(shape: &Shape, ox: f64, oy: f64) -> Option<tiny_skia::Path> {
    let mut pb = tiny_skia::PathBuilder::new();
    for contour in shape.contours.iter() {
        let mut points = contour
            .iter()
            .map(|&[x, y]| ((x - ox) as f32, (y - oy) as f32));
        let Some((x, y)) = points.next() else {
            continue;
        };
        pb.move_to(x, y);
        for (x, y) in points {
            pb.line_to(x, y);
        }
        pb.close();
    }
    pb.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgba<u8> = Rgba([255, 0, 0, 255]);
    const BLUE: Rgba<u8> = Rgba([0, 0, 255, 255]);

    fn fill(color: Rgba<u8>) -> Style {
        Style {
            fill: color,
            ..Style::default()
        }
    }

    /// The color at parent pixel (x, y) once rasterized, transparent outside.
    fn pixel_at(shape: &Shape, x: i64, y: i64) -> [u8; 4] {
        let (img, x0, y0) = rasterize(shape);
        let (px, py) = (x - x0, y - y0);
        if px < 0 || py < 0 || px >= i64::from(img.width()) || py >= i64::from(img.height()) {
            return [0; 4];
        }
        img.get_pixel(px as u32, py as u32).0
    }

    fn area(shape: &Shape) -> f64 {
        // Shoelace; even-odd contours from the overlay never overlap, holes wind the other
        // way, so the signed sum is the covered area.
        shape
            .contours
            .iter()
            .map(|c| {
                let n = c.len();
                (0..n)
                    .map(|i| {
                        let [x0, y0] = c[i];
                        let [x1, y1] = c[(i + 1) % n];
                        x0 * y1 - x1 * y0
                    })
                    .sum::<f64>()
                    / 2.0
            })
            .sum::<f64>()
            .abs()
    }

    #[test]
    fn rectangle_fills_its_box_exactly() {
        let s = Shape::rectangle(4.0, 2.0, 0.0, fill(RED));
        assert_eq!(s.contours.len(), 1);
        assert_eq!(s.contours[0].len(), 4);
        // Edges on whole pixels: no margin, no soft edge.
        let (img, x0, y0) = rasterize(&s);
        assert_eq!((img.dimensions(), x0, y0), ((4, 2), 0, 0));
        assert!(img.pixels().all(|p| *p == RED));
        assert_eq!(pixel_at(&s, 0, 0), RED.0);
        assert_eq!(pixel_at(&s, 3, 1), RED.0);
        assert_eq!(pixel_at(&s, 4, 1), [0; 4]);
        assert_eq!(pixel_at(&s, -1, 0), [0; 4]);
    }

    #[test]
    fn rounded_corners_and_ovals_leave_their_corners_clear() {
        let r = Shape::rectangle(20.0, 20.0, 8.0, fill(RED));
        assert_eq!(pixel_at(&r, 0, 0)[3], 0);
        assert_eq!(pixel_at(&r, 10, 10), RED.0);
        assert_eq!(pixel_at(&r, 10, 0), RED.0);

        let o = Shape::oval(20.0, 10.0, fill(RED));
        assert_eq!(pixel_at(&o, 1, 1)[3], 0);
        assert_eq!(pixel_at(&o, 10, 5), RED.0);
        let want = std::f64::consts::PI * 10.0 * 5.0;
        // Chords sit just inside the curve, so the flattened area is a touch smaller.
        let got = area(&o);
        assert!(
            got < want && got > want * 0.99,
            "area {got}, want about {want}"
        );
    }

    #[test]
    fn corner_radius_is_clamped() {
        let r = Shape::rectangle(10.0, 4.0, 100.0, fill(RED));
        let (x0, y0, x1, y1) = r.outline_bounds().unwrap();
        assert!(
            (x1 - x0 - 10.0).abs() < 1e-9 && (y1 - y0 - 4.0).abs() < 1e-9 && x0 == 0.0 && y0 == 0.0
        );
    }

    #[test]
    fn placed_moves_and_rotates_about_the_centre() {
        let s = Shape::rectangle(10.0, 2.0, 0.0, fill(RED)).placed(100.0, 50.0, 90.0, 10.0, 2.0);
        let (x0, y0, x1, y1) = s.outline_bounds().unwrap();
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        // Centre stays at (105, 51); now 2 wide and 10 tall.
        assert!(close(x0, 104.0) && close(x1, 106.0) && close(y0, 46.0) && close(y1, 56.0));
    }

    #[test]
    fn booleans() {
        let a = Shape::rectangle(10.0, 10.0, 0.0, fill(RED));
        let b = Shape::rectangle(10.0, 10.0, 0.0, fill(BLUE)).placed(5.0, 0.0, 0.0, 10.0, 10.0);
        let close = |s: Shape, want: f64| {
            let got = area(&s);
            assert!((got - want).abs() < 1e-6, "area {got}, want {want}");
            s
        };
        let u = close(a.combine(&b, BooleanOp::Union), 150.0);
        assert_eq!(u.style.fill, RED, "keeps the first shape's style");
        close(a.combine(&b, BooleanOp::Subtract), 50.0);
        close(a.combine(&b, BooleanOp::Intersect), 50.0);
        let x = close(a.combine(&b, BooleanOp::Exclude), 100.0);
        assert_eq!(pixel_at(&x, 7, 5)[3], 0, "the overlap is cleared");
        assert_eq!(pixel_at(&x, 2, 5), RED.0);
        assert_eq!(pixel_at(&x, 12, 5), RED.0);
    }

    #[test]
    fn subtract_can_leave_a_hole() {
        let a = Shape::rectangle(10.0, 10.0, 0.0, fill(RED));
        let hole = Shape::oval(4.0, 4.0, fill(BLUE)).placed(3.0, 3.0, 0.0, 4.0, 4.0);
        let s = a.combine(&hole, BooleanOp::Subtract);
        assert_eq!(s.contours.len(), 2);
        assert_eq!(pixel_at(&s, 5, 5)[3], 0);
        assert_eq!(pixel_at(&s, 1, 1), RED.0);
    }

    #[test]
    fn disjoint_intersection_is_empty_and_draws_nothing() {
        let a = Shape::rectangle(2.0, 2.0, 0.0, fill(RED));
        let b = a.placed(10.0, 10.0, 0.0, 2.0, 2.0);
        let s = a.combine(&b, BooleanOp::Intersect);
        assert!(s.contours.is_empty());
        assert_eq!(rasterize(&s).0.dimensions(), (0, 0));
        // And empty shapes combine sensibly.
        assert_eq!(s.combine(&a, BooleanOp::Union).contours, a.contours);
        assert!(s.combine(&a, BooleanOp::Subtract).contours.is_empty());
    }

    #[test]
    fn stroke_paints_around_the_outline_and_grows_the_bounds() {
        let style = Style {
            fill: Rgba([0, 0, 0, 0]),
            stroke: BLUE,
            stroke_width: 4.0,
            ..Style::default()
        };
        let s = Shape::rectangle(20.0, 20.0, 0.0, style);
        assert_eq!(s.raster_bounds(), Some((-2, -2, 22, 22)));
        assert_eq!(pixel_at(&s, 10, 0), BLUE.0, "on the edge");
        assert_eq!(pixel_at(&s, 10, -2), BLUE.0, "outside half");
        assert_eq!(pixel_at(&s, -2, -2), BLUE.0, "mitered corner");
        assert_eq!(pixel_at(&s, 10, 10)[3], 0, "fill is transparent");
    }

    #[test]
    fn opacity_scales_alpha() {
        let style = Style {
            fill: Rgba([255, 0, 0, 200]),
            fill_opacity: 0.5,
            ..Style::default()
        };
        let s = Shape::rectangle(4.0, 4.0, 0.0, style);
        let [r, g, b, a] = pixel_at(&s, 1, 1);
        assert_eq!((g, b, a), (0, 0, 100));
        assert!(r >= 250, "straight alpha, so the color survives: {r}");
    }
}
