//! `font`: loads a font file from the project folder, to link into `text`'s `font`.

use std::path::PathBuf;

use conduit_core::{
    EvalCtx, FontData, Inputs, Node, NodeError, NodeResult, NodeSpec, Outputs, ParamSpec, PortSpec,
    Value, ValueType, parse_params,
};
use cosmic_text::fontdb::{Database, Source};
use serde::Deserialize;

use crate::image::check_path;

/// A TrueType / OpenType font file from the project folder.
pub struct Font;

/// File extensions `font` reads (editors list files with these).
pub const EXTENSIONS: &[&str] = &["ttf", "otf", "ttc"];

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

/// The family names in a font file, or an error if it isn't one.
pub fn families(font: &FontData) -> NodeResult<Vec<String>> {
    let mut db = Database::new();
    db.load_font_source(Source::Binary(font.data.clone()));
    let mut names: Vec<String> = db
        .faces()
        .filter_map(|f| f.families.first().map(|(name, _)| name.clone()))
        .collect();
    names.dedup();
    if names.is_empty() {
        return Err(NodeError::new(format!(
            "`{}` is not a TrueType or OpenType font",
            font.name
        )));
    }
    Ok(names)
}

impl Node for Font {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "font",
            inputs: vec![],
            outputs: vec![PortSpec::new("out", ValueType::Font)],
            params: vec![
                ParamSpec::file("path", EXTENSIONS, "\"assets/fonts/my_font.ttf\"")
                    .doc("font file (.ttf, .otf, .ttc), relative to the project folder"),
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
        let data = std::fs::read(ctx.root.join(&p.path))
            .map_err(|e| NodeError::new(format!("cannot read `{}`: {e}", p.path)))?;
        let font = FontData::new(p.path, data);
        families(&font)?;
        Ok(Outputs::out(Value::Font(font)))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use super::*;
    use crate::test_util::{params, run_in};

    const DROID: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/fonts/DroidSans.ttf"
    );

    fn eval(root: &Path, path: &str) -> NodeResult<FontData> {
        match run_in(root, &Font, vec![], &params(&format!("path = \"{path}\"")))? {
            Value::Font(f) => Ok(f),
            other => panic!("expected font, got {}", other.ty()),
        }
    }

    #[test]
    fn loads_a_font_relative_to_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("assets/fonts")).unwrap();
        std::fs::copy(DROID, dir.path().join("assets/fonts/my_font.ttf")).unwrap();
        let font = eval(dir.path(), "assets/fonts/my_font.ttf").unwrap();
        assert_eq!(font.name, "assets/fonts/my_font.ttf");
        assert_eq!(families(&font).unwrap(), ["Droid Sans"]);
        assert_eq!(Arc::strong_count(&font.data), 1);
    }

    #[test]
    fn reports_missing_files_and_non_fonts() {
        let dir = tempfile::tempdir().unwrap();
        let err = eval(dir.path(), "nope.ttf").err().unwrap().0;
        assert!(err.starts_with("cannot read `nope.ttf`"), "{err}");
        std::fs::write(dir.path().join("fake.ttf"), "not a font").unwrap();
        let err = eval(dir.path(), "fake.ttf").err().unwrap().0;
        assert_eq!(err, "`fake.ttf` is not a TrueType or OpenType font");
    }

    #[test]
    fn paths_stay_inside_the_project() {
        let check = |s: &str| Font.check_params(&params(s));
        assert!(check("path = \"assets/fonts/a.ttf\"").is_ok());
        assert!(check("path = \"../a.ttf\"").is_err());
        assert!(check("path = \"/usr/share/fonts/a.ttf\"").is_err());
        assert!(check("").is_err(), "path is required");
        assert_eq!(
            Font.files(&params("path = \"assets/fonts/a.ttf\"")),
            [PathBuf::from("assets/fonts/a.ttf")]
        );
    }
}
