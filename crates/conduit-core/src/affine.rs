//! 2D affine transforms that place a layer's pixels in its parent canvas.

use std::fmt;

use serde::Deserialize;

/// How close a matrix entry must be to a whole number to count as one. Absorbs rounding
/// from trigonometry, e.g. a 360° rotation.
const SNAP: f64 = 1e-9;

/// A 2D affine transform, mapping a point (x, y) to
/// (a·x + c·y + e, b·x + d·y + f). Points are in pixels with y pointing down, so a
/// positive rotation turns clockwise on screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    pub fn translate(x: f64, y: f64) -> Self {
        Self {
            e: x,
            f: y,
            ..Self::IDENTITY
        }
    }

    pub fn scale(x: f64, y: f64) -> Self {
        Self {
            a: x,
            d: y,
            ..Self::IDENTITY
        }
    }

    /// Clockwise rotation by `degrees` (y points down).
    pub fn rotate(degrees: f64) -> Self {
        let (sin, cos) = degrees.to_radians().sin_cos();
        Self {
            a: cos,
            b: sin,
            c: -sin,
            d: cos,
            ..Self::IDENTITY
        }
    }

    /// Shear: `x_degrees` slants vertical edges (x moves with y), `y_degrees` slants
    /// horizontal edges (y moves with x).
    pub fn skew(x_degrees: f64, y_degrees: f64) -> Self {
        Self {
            b: y_degrees.to_radians().tan(),
            c: x_degrees.to_radians().tan(),
            ..Self::IDENTITY
        }
    }

    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.a * x + self.c * y + self.e,
            self.b * x + self.d * y + self.f,
        )
    }

    /// The inverse transform, or `None` if this one flattens the plane (e.g. scale 0).
    pub fn invert(&self) -> Option<Self> {
        let det = self.a * self.d - self.b * self.c;
        if !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        let (a, b, c, d) = (self.d / det, -self.b / det, -self.c / det, self.a / det);
        Some(Self {
            a,
            b,
            c,
            d,
            e: -(a * self.e + c * self.f),
            f: -(b * self.e + d * self.f),
        })
    }

    /// `Some((x, y))` if this only moves by whole pixels, so pixels can be copied as-is.
    pub fn whole_pixel_offset(&self) -> Option<(i64, i64)> {
        let near = |v: f64, target: f64| (v - target).abs() < SNAP;
        let whole = |v: f64| near(v, v.round()) && v.abs() < 1e15;
        (near(self.a, 1.0)
            && near(self.b, 0.0)
            && near(self.c, 0.0)
            && near(self.d, 1.0)
            && whole(self.e)
            && whole(self.f))
        .then(|| (self.e.round() as i64, self.f.round() as i64))
    }

    pub fn is_finite(&self) -> bool {
        [self.a, self.b, self.c, self.d, self.e, self.f]
            .iter()
            .all(|v| v.is_finite())
    }
}

/// `m * n` applies `n` first, then `m` (matrix product).
impl std::ops::Mul for Affine {
    type Output = Affine;

    fn mul(self, o: Affine) -> Affine {
        let s = self;
        Affine {
            a: s.a * o.a + s.c * o.b,
            b: s.b * o.a + s.d * o.b,
            c: s.a * o.c + s.c * o.d,
            d: s.b * o.c + s.d * o.d,
            e: s.a * o.e + s.c * o.f + s.e,
            f: s.b * o.e + s.d * o.f + s.f,
        }
    }
}

impl Default for Affine {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// How a transformed layer's pixels are sampled when it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Filter {
    /// Hard pixel edges; good for pixel art.
    Nearest,
    /// Smooth, slightly soft.
    #[default]
    Bilinear,
    /// Sharper than bilinear (Catmull-Rom).
    Bicubic,
}

impl fmt::Display for Filter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Filter::Nearest => "nearest",
            Filter::Bilinear => "bilinear",
            Filter::Bicubic => "bicubic",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(p: (f64, f64), q: (f64, f64)) -> bool {
        (p.0 - q.0).abs() < 1e-9 && (p.1 - q.1).abs() < 1e-9
    }

    #[test]
    fn rotation_is_clockwise_with_y_down() {
        // Pointing right, then a quarter turn clockwise points down.
        assert!(close(Affine::rotate(90.0).apply(1.0, 0.0), (0.0, 1.0)));
    }

    #[test]
    fn product_applies_the_right_side_first() {
        let m = Affine::translate(10.0, 0.0) * Affine::scale(2.0, 2.0);
        assert!(close(m.apply(1.0, 1.0), (12.0, 2.0)));
    }

    #[test]
    fn inverse_round_trips() {
        let m = Affine::translate(3.0, -4.0)
            * Affine::rotate(30.0)
            * Affine::skew(10.0, 0.0)
            * Affine::scale(2.0, 0.5);
        let p = m.apply(5.0, 7.0);
        assert!(close(m.invert().unwrap().apply(p.0, p.1), (5.0, 7.0)));
        assert!(Affine::scale(0.0, 1.0).invert().is_none());
    }

    #[test]
    fn whole_pixel_offsets() {
        assert_eq!(
            Affine::translate(3.0, -2.0).whole_pixel_offset(),
            Some((3, -2))
        );
        assert_eq!(Affine::translate(0.5, 0.0).whole_pixel_offset(), None);
        assert_eq!(Affine::rotate(90.0).whole_pixel_offset(), None);
        // A full turn is the identity, give or take rounding.
        assert_eq!(Affine::rotate(360.0).whole_pixel_offset(), Some((0, 0)));
    }
}
