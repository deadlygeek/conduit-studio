//! Window-independent preview logic: zoom levels and PNG saving.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MIN_SCALE: f32 = 1.0 / 32.0;
pub const MAX_SCALE: f32 = 32.0;
const ZOOM_STEP: f32 = 1.25;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Zoom {
    /// Scale the whole image to fit the window.
    Fit,
    /// Fixed scale (1.0 = one image pixel per screen point).
    Scale(f32),
}

impl Zoom {
    /// The scale actually used for an image of `image` size shown in `available` space.
    pub fn scale(self, image: [f32; 2], available: [f32; 2]) -> f32 {
        match self {
            Zoom::Fit => {
                let s = (available[0] / image[0]).min(available[1] / image[1]);
                if s.is_finite() {
                    s.clamp(MIN_SCALE, MAX_SCALE)
                } else {
                    1.0
                }
            }
            Zoom::Scale(s) => s,
        }
    }

    pub fn zoom_in(current_scale: f32) -> Zoom {
        Zoom::Scale((current_scale * ZOOM_STEP).min(MAX_SCALE))
    }

    pub fn zoom_out(current_scale: f32) -> Zoom {
        Zoom::Scale((current_scale / ZOOM_STEP).max(MIN_SCALE))
    }
}

/// Saves straight-alpha RGBA pixels as `<dir>/<node_id>-<unix seconds>[-n].png`, never
/// overwriting an existing file. Returns the path written.
pub fn save_png(
    dir: &Path,
    node_id: &str,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let path = (0..)
        .map(|n| match n {
            0 => dir.join(format!("{node_id}-{secs}.png")),
            n => dir.join(format!("{node_id}-{secs}-{n}.png")),
        })
        .find(|p| !p.exists())
        .expect("unbounded range");
    image::save_buffer(&path, rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| format!("cannot save {}: {e}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_uses_the_tighter_axis() {
        assert_eq!(Zoom::Fit.scale([800.0, 600.0], [400.0, 600.0]), 0.5);
        assert_eq!(Zoom::Fit.scale([100.0, 100.0], [300.0, 200.0]), 2.0);
        assert_eq!(Zoom::Fit.scale([0.0, 0.0], [300.0, 200.0]), 1.0);
        assert_eq!(Zoom::Scale(3.0).scale([1.0, 1.0], [1.0, 1.0]), 3.0);
    }

    #[test]
    fn zoom_steps_are_clamped() {
        assert_eq!(Zoom::zoom_in(1.0), Zoom::Scale(1.25));
        assert_eq!(Zoom::zoom_out(1.25), Zoom::Scale(1.0));
        assert_eq!(Zoom::zoom_in(MAX_SCALE), Zoom::Scale(MAX_SCALE));
        assert_eq!(Zoom::zoom_out(MIN_SCALE), Zoom::Scale(MIN_SCALE));
    }

    #[test]
    fn saves_png_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out");
        let rgba = [255, 0, 0, 255, 0, 0, 255, 128];
        let a = save_png(&out, "prev", 2, 1, &rgba).unwrap();
        let b = save_png(&out, "prev", 2, 1, &rgba).unwrap();
        assert_ne!(a, b);
        let img = image::open(&a).unwrap().to_rgba8();
        assert_eq!(img.dimensions(), (2, 1));
        assert_eq!(img.as_raw().as_slice(), rgba);
    }
}
