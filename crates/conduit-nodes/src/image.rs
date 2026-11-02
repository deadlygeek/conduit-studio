//! `image`: loads a picture from the project folder (PNG, JPEG, WebP, GIF or BMP).

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use conduit_core::image::{DynamicImage, ImageDecoder, ImageReader, Limits};
use conduit_core::{
    EvalCtx, Inputs, Node, NodeError, NodeResult, NodeSpec, Outputs, ParamSpec, PortSpec, Value,
    ValueType, parse_params,
};
use serde::Deserialize;

use crate::MAX_DIMENSION;

/// An image file from the project folder, at its own size.
pub struct Image;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    path: String,
}

impl Params {
    fn parse(params: &toml::Table) -> NodeResult<Self> {
        let p: Self = parse_params(params)?;
        check_path(&p.path)?;
        Ok(p)
    }
}

/// Paths stay inside the project folder, so a project can be moved or shared as a whole.
fn check_path(path: &str) -> NodeResult<()> {
    let inside = Path::new(path)
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if path.trim().is_empty() {
        Err(NodeError::new("`path` is empty"))
    } else if !inside {
        Err(NodeError::new(format!(
            "`path` must be relative to the project folder and stay inside it, got `{path}`"
        )))
    } else {
        Ok(())
    }
}

fn read(file: &Path, shown: &str) -> NodeResult<DynamicImage> {
    let error = |e: &dyn std::fmt::Display| NodeError::new(format!("cannot read `{shown}`: {e}"));
    let mut reader = ImageReader::open(file)
        .map_err(|e| error(&e))?
        .with_guessed_format()
        .map_err(|e| error(&e))?;
    if reader.format().is_none() {
        return Err(NodeError::new(format!(
            "cannot read `{shown}`: not a PNG, JPEG, WebP, GIF or BMP image"
        )));
    }
    // Refuse huge images before decoding allocates them.
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|e| error(&e))?;
    // Photos are often stored sideways with an EXIF tag saying which way is up.
    let orientation = decoder.orientation().map_err(|e| error(&e))?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(|e| error(&e))?;
    image.apply_orientation(orientation);
    Ok(image)
}

impl Node for Image {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "image",
            inputs: vec![],
            outputs: vec![PortSpec::new("out", ValueType::Image)],
            params: vec![
                ParamSpec::text("path", "\"assets/images/cat.jpg\"")
                    .doc("image file, relative to the project folder"),
            ],
            gui: false,
        }
    }

    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        Params::parse(params).map(|_| ())
    }

    fn files(&self, params: &toml::Table) -> Vec<PathBuf> {
        Params::parse(params)
            .map(|p| vec![PathBuf::from(p.path)])
            .unwrap_or_default()
    }

    fn eval(&self, _: &Inputs, params: &toml::Table, ctx: &mut EvalCtx) -> NodeResult<Outputs> {
        let p = Params::parse(params)?;
        let image = read(&ctx.root.join(&p.path), &p.path)?.into_rgba8();
        Ok(Outputs::out(Value::Image(Arc::new(image))))
    }
}

#[cfg(test)]
mod tests {
    use conduit_core::{Rgba, RgbaImage};

    use super::*;
    use crate::test_util::{params, run_in};

    fn eval(root: &Path, path: &str) -> NodeResult<Arc<RgbaImage>> {
        match run_in(root, &Image, vec![], &params(&format!("path = \"{path}\"")))? {
            Value::Image(img) => Ok(img),
            other => panic!("expected image, got {}", other.ty()),
        }
    }

    #[test]
    fn loads_png_and_jpeg_relative_to_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("assets/images")).unwrap();
        let mut png = RgbaImage::from_pixel(3, 2, Rgba([10, 20, 30, 128]));
        png.put_pixel(2, 1, Rgba([255, 0, 0, 255]));
        png.save(dir.path().join("assets/images/a.png")).unwrap();
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 5, Rgba([0, 0, 255, 255])))
            .into_rgb8()
            .save(dir.path().join("assets/images/cat.jpg"))
            .unwrap();

        assert_eq!(*eval(dir.path(), "assets/images/a.png").unwrap(), png);

        let jpg = eval(dir.path(), "./assets/images/cat.jpg").unwrap();
        assert_eq!(jpg.dimensions(), (4, 5));
        let [r, g, b, a] = jpg.get_pixel(1, 1).0;
        assert!(r < 10 && g < 10 && b > 245 && a == 255, "{r} {g} {b} {a}");
    }

    #[test]
    fn reports_missing_and_unreadable_files() {
        let dir = tempfile::tempdir().unwrap();
        let err = eval(dir.path(), "nope.png").unwrap_err().0;
        assert!(err.starts_with("cannot read `nope.png`"), "{err}");

        // A known extension is decoded as that format; anything else must be recognizable.
        std::fs::write(dir.path().join("notes.png"), "not an image").unwrap();
        let err = eval(dir.path(), "notes.png").unwrap_err().0;
        assert!(err.contains("Invalid PNG signature"), "{err}");
        std::fs::write(dir.path().join("notes.txt"), "not an image").unwrap();
        let err = eval(dir.path(), "notes.txt").unwrap_err().0;
        assert!(err.contains("not a PNG, JPEG"), "{err}");
    }

    #[test]
    fn paths_stay_inside_the_project() {
        let check = |s: &str| Image.check_params(&params(s));
        assert!(check("path = \"assets/cat.jpg\"").is_ok());
        assert!(check("path = \"../cat.jpg\"").is_err());
        assert!(check("path = \"assets/../../cat.jpg\"").is_err());
        assert!(check("path = \"/etc/cat.jpg\"").is_err());
        assert!(check("path = \"\"").is_err());
        assert!(check("").is_err(), "path is required");
        assert!(check("path = \"a.png\"\nwidth = 3").is_err());
        assert_eq!(
            Image.files(&params("path = \"assets/cat.jpg\"")),
            [PathBuf::from("assets/cat.jpg")]
        );
        assert!(Image.files(&params("path = \"../x.png\"")).is_empty());
    }
}
