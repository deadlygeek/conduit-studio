//! The `text` node: text set in an installed font, as a layer.

use std::sync::{Arc, Mutex, OnceLock};

use conduit_core::{
    Color, EvalCtx, Inputs, Layer, Node, NodeError, NodeResult, NodeSpec, Outputs, ParamSpec,
    PortSpec, Rgba, RgbaImage, Value, ValueType, parse_params, raster,
};
use cosmic_text::fontdb::{self, Database};
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, Weight};
use serde::Deserialize;

use crate::MAX_DIMENSION;

/// Line spacing as a multiple of the font size.
const LINE_HEIGHT: f32 = 1.2;
const MAX_SIZE: f64 = 2048.0;

/// Renders text with an installed font. The layer's origin is the top-left of the text
/// box (the lines' boxes, as in CSS), so a `transform` positions and pivots it
/// predictably; glyphs that poke outside the box (italic overhangs, accents) are kept.
pub struct Text {
    fonts: OnceLock<Mutex<Fonts>>,
}

struct Fonts {
    system: FontSystem,
    cache: SwashCache,
}

impl Text {
    /// Uses the fonts installed on this machine, loaded on first use.
    pub fn system() -> Self {
        Self {
            fonts: OnceLock::new(),
        }
    }

    /// Uses only the fonts in `db` (e.g. fonts bundled with tests).
    pub fn with_fonts(db: Database) -> Self {
        let text = Self::system();
        let _ = text.fonts.set(Mutex::new(Fonts::new(db)));
        text
    }

    fn fonts(&self) -> &Mutex<Fonts> {
        self.fonts
            .get_or_init(|| Mutex::new(Fonts::new(system_fonts())))
    }
}

impl Default for Text {
    fn default() -> Self {
        Self::system()
    }
}

impl Fonts {
    fn new(mut db: Database) -> Self {
        resolve_generic_families(&mut db);
        Self {
            system: FontSystem::new_with_locale_and_db("en-US".into(), db),
            cache: SwashCache::new(),
        }
    }
}

fn system_fonts() -> Database {
    let mut db = Database::new();
    db.load_system_fonts();
    db
}

/// Names `font` accepts besides installed families.
pub const GENERIC_FAMILIES: [&str; 3] = ["sans-serif", "serif", "monospace"];

/// An installed font family, as offered by `conduit fonts` and the editor's font picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFamily {
    pub name: String,
    /// Every glyph is the same width.
    pub monospace: bool,
    /// Has an italic or oblique face (otherwise `style = "italic"` falls back).
    pub italic: bool,
}

/// Every installed font family, sorted by name (case-insensitive). Loading system fonts
/// takes a moment, so callers should keep the result.
pub fn font_families() -> Vec<FontFamily> {
    let mut by_name: std::collections::BTreeMap<String, FontFamily> = Default::default();
    for face in system_fonts().faces() {
        // The first name is the English one when the font has several.
        let Some((name, _)) = face.families.first() else {
            continue;
        };
        let family = by_name
            .entry(name.to_lowercase())
            .or_insert_with(|| FontFamily {
                name: name.clone(),
                monospace: false,
                italic: false,
            });
        family.monospace |= face.monospaced;
        family.italic |= face.style != fontdb::Style::Normal;
    }
    by_name.into_values().collect()
}

/// Generic family names accepted by `font`, with the installed families tried for each,
/// in order. cosmic-text's own defaults name fonts that are often not installed.
const GENERIC: [(&str, &[&str]); 3] = [
    (
        "sans-serif",
        &[
            "Open Sans",
            "Noto Sans",
            "DejaVu Sans",
            "Adwaita Sans",
            "Cantarell",
            "Liberation Sans",
            "Arial",
            "Helvetica",
            "Droid Sans",
        ],
    ),
    (
        "serif",
        &[
            "Noto Serif",
            "DejaVu Serif",
            "Liberation Serif",
            "Times New Roman",
            "C059",
            "Georgia",
        ],
    ),
    (
        "monospace",
        &[
            "Noto Sans Mono",
            "DejaVu Sans Mono",
            "Adwaita Mono",
            "Liberation Mono",
            "Cascadia Code",
            "Courier New",
        ],
    ),
];

/// Points each generic family at the first preferred family that's installed, else at any
/// installed family, so a generic name always finds a font.
fn resolve_generic_families(db: &mut Database) {
    let Some(any) = installed_families(db).next().map(str::to_string) else {
        return;
    };
    for (generic, preferred) in GENERIC {
        let pick = preferred
            .iter()
            .find_map(|p| find_family(db, p))
            .unwrap_or_else(|| any.clone());
        match generic {
            "sans-serif" => db.set_sans_serif_family(pick),
            "serif" => db.set_serif_family(pick),
            _ => db.set_monospace_family(pick),
        }
    }
}

fn installed_families(db: &Database) -> impl Iterator<Item = &str> {
    db.faces()
        .flat_map(|f| f.families.iter().map(|(name, _)| name.as_str()))
}

/// The installed family matching `name` (ignoring case), spelled as installed.
fn find_family(db: &Database, name: &str) -> Option<String> {
    installed_families(db)
        .find(|f| f.eq_ignore_ascii_case(name))
        .map(str::to_string)
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
enum Style {
    #[default]
    Normal,
    Italic,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    text: String,
    #[serde(default = "default_font")]
    font: String,
    #[serde(default = "default_size")]
    size: f64,
    #[serde(default = "default_color")]
    color: Color,
    #[serde(default = "default_weight")]
    weight: u16,
    #[serde(default)]
    style: Style,
}

fn default_font() -> String {
    "sans-serif".into()
}

fn default_size() -> f64 {
    48.0
}

fn default_color() -> Color {
    Color(Rgba([0, 0, 0, 255]))
}

fn default_weight() -> u16 {
    400
}

impl Params {
    fn parse(params: &toml::Table) -> NodeResult<Self> {
        let p: Self = parse_params(params)?;
        if !(1.0..=MAX_SIZE).contains(&p.size) {
            return Err(NodeError::new(format!(
                "`size` must be between 1 and {MAX_SIZE}, got {}",
                p.size
            )));
        }
        if !(100..=900).contains(&p.weight) {
            return Err(NodeError::new(format!(
                "`weight` must be between 100 and 900, got {}",
                p.weight
            )));
        }
        Ok(p)
    }
}

impl Node for Text {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "text",
            inputs: vec![],
            outputs: vec![PortSpec::new("out", ValueType::Layer)],
            params: vec![
                ParamSpec::text("text", "\"Hello\"").doc("the text; \\n starts a new line"),
                ParamSpec::font("font", "\"sans-serif\"")
                    .default("\"sans-serif\"")
                    .doc("an installed family (see `conduit fonts`), or serif / sans-serif / monospace"),
                ParamSpec::float("size", 1.0, MAX_SIZE, "48")
                    .default("48")
                    .doc("font size in pixels"),
                ParamSpec::color("color", "\"black\"")
                    .default("\"black\"")
                    .doc("text color"),
                ParamSpec::int("weight", 100, 900, "400")
                    .default("400")
                    .doc("400 regular, 700 bold"),
                ParamSpec::text("style", "\"normal\"")
                    .default("\"normal\"")
                    .doc("normal or italic"),
            ],
            gui: false,
        }
    }

    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        Params::parse(params).map(|_| ())
    }

    fn eval(&self, _: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let p = Params::parse(params)?;
        let mut fonts = self.fonts().lock().unwrap_or_else(|e| e.into_inner());
        let layer = render(&mut fonts, &p)?;
        Ok(Outputs::out(Value::Layer(layer)))
    }
}

fn render(fonts: &mut Fonts, p: &Params) -> NodeResult<Layer> {
    let Fonts { system, cache } = fonts;
    let generic = p.font.to_ascii_lowercase();
    let named = match generic.as_str() {
        "sans-serif" | "serif" | "monospace" => None,
        _ => Some(find_family(system.db(), &p.font).ok_or_else(|| {
            NodeError::new(format!(
                "no installed font family \"{}\" (run `conduit fonts` to list them)",
                p.font
            ))
        })?),
    };
    let family = match (named.as_deref(), generic.as_str()) {
        (Some(name), _) => Family::Name(name),
        (None, "serif") => Family::Serif,
        (None, "monospace") => Family::Monospace,
        (None, _) => Family::SansSerif,
    };
    if p.text.is_empty() {
        return Ok(Layer::new(Arc::new(RgbaImage::new(0, 0))));
    }

    let size = p.size as f32;
    let mut buffer = Buffer::new(system, Metrics::new(size, size * LINE_HEIGHT));
    let attrs = Attrs::new()
        .family(family)
        .weight(Weight(p.weight))
        .style(match p.style {
            Style::Normal => fontdb::Style::Normal,
            Style::Italic => fontdb::Style::Italic,
        });
    buffer.set_size(None, None);
    buffer.set_text(&p.text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(system, false);

    // The text box: the widest line by the lines' total height.
    let (mut box_w, mut box_h) = (0.0f32, 0.0f32);
    for run in buffer.layout_runs() {
        box_w = box_w.max(run.line_w);
        box_h = box_h.max(run.line_top + run.line_height);
    }

    // Collect glyph pixels first: the ink can reach outside the box.
    let [r, g, b, a] = p.color.0.0;
    let mut pixels = Vec::new();
    buffer.draw(
        system,
        cache,
        cosmic_text::Color::rgba(r, g, b, a),
        |x, y, w, h, c| {
            if c.a() == 0 {
                return;
            }
            for dy in 0..h as i32 {
                for dx in 0..w as i32 {
                    pixels.push((x + dx, y + dy, c));
                }
            }
        },
    );

    let (mut x0, mut y0) = (0i64, 0i64);
    let (mut x1, mut y1) = (box_w.ceil() as i64, box_h.ceil() as i64);
    for &(x, y, _) in &pixels {
        let (x, y) = (i64::from(x), i64::from(y));
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x + 1);
        y1 = y1.max(y + 1);
    }
    let (w, h) = (x1 - x0, y1 - y0);
    if w > i64::from(MAX_DIMENSION) || h > i64::from(MAX_DIMENSION) {
        return Err(NodeError::new(format!(
            "the text would be {w}x{h} pixels, larger than the {MAX_DIMENSION}x{MAX_DIMENSION} limit"
        )));
    }

    let mut image = RgbaImage::new(w as u32, h as u32);
    for (x, y, c) in pixels {
        let px = image.get_pixel_mut((i64::from(x) - x0) as u32, (i64::from(y) - y0) as u32);
        *px = raster::blend_over(Rgba(c.as_rgba()), *px);
    }
    Ok(Layer::at(Arc::new(image), x0, y0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{params, run};

    fn fixture_fonts() -> Text {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fonts");
        let mut db = Database::new();
        db.load_fonts_dir(dir);
        assert!(db.len() >= 2, "bundled fonts missing from {dir}");
        Text::with_fonts(db)
    }

    fn text(node: &Text, toml: &str) -> NodeResult<Layer> {
        match run(node, vec![], &params(toml))? {
            Value::Layer(l) => Ok(l),
            other => panic!("expected layer, got {other:?}"),
        }
    }

    fn ink(layer: &Layer) -> usize {
        layer.image.pixels().filter(|p| p[3] > 0).count()
    }

    #[test]
    fn renders_in_the_requested_color() {
        let node = fixture_fonts();
        let layer = text(&node, "text = \"Hello\"\ncolor = \"#e76f51\"").unwrap();
        assert!(ink(&layer) > 100);
        // Only alpha varies (anti-aliasing); the color is exact everywhere.
        assert!(
            layer
                .image
                .pixels()
                .filter(|p| p[3] > 0)
                .all(|p| p.0[..3] == [0xe7, 0x6f, 0x51])
        );
    }

    #[test]
    fn origin_is_the_top_left_of_the_text_box() {
        let node = fixture_fonts();
        let layer = text(&node, "text = \"Hello\"\nsize = 40").unwrap();
        let (x, y) = layer.transform.whole_pixel_offset().unwrap();
        assert!(x <= 0 && y <= 0, "offset {x},{y}");
        // One line is about 1.2 × size tall.
        let h = layer.image.height() as i64 + y;
        assert!((46..=50).contains(&h), "height {h}");
    }

    #[test]
    fn size_lines_and_weight_change_the_result() {
        let node = fixture_fonts();
        let small = text(&node, "text = \"Hello\"\nsize = 20").unwrap();
        let big = text(&node, "text = \"Hello\"\nsize = 40").unwrap();
        let ratio = big.image.width() as f64 / small.image.width() as f64;
        assert!((1.8..=2.2).contains(&ratio), "width ratio {ratio}");

        let two = text(&node, "text = \"Hello\\nworld\"\nsize = 20").unwrap();
        assert!(two.image.height() > small.image.height() * 3 / 2);

        let bold = text(&node, "text = \"Hello\"\nsize = 20\nweight = 700").unwrap();
        assert!(ink(&bold) > ink(&small), "bold has more ink");
    }

    #[test]
    fn families_match_by_name_and_generics_always_resolve() {
        let node = fixture_fonts();
        assert!(text(&node, "text = \"a\"\nfont = \"droid sans\"").is_ok());
        for generic in ["serif", "sans-serif", "monospace"] {
            let layer = text(&node, &format!("text = \"a\"\nfont = \"{generic}\"")).unwrap();
            assert!(ink(&layer) > 0, "{generic}");
        }
        let err = text(&node, "text = \"a\"\nfont = \"Nope Sans\"")
            .err()
            .unwrap();
        assert_eq!(
            err.0,
            "no installed font family \"Nope Sans\" (run `conduit fonts` to list them)"
        );
    }

    #[test]
    fn empty_text_draws_nothing_and_bad_params_are_rejected() {
        let node = fixture_fonts();
        assert_eq!(
            text(&node, "text = \"\"").unwrap().image.dimensions(),
            (0, 0)
        );
        for bad in [
            "text = \"a\"\nstyle = \"oblique\"",
            "text = \"a\"\nweight = 1000",
            "text = \"a\"\nsize = 0",
            "size = 10",
        ] {
            assert!(node.check_params(&params(bad)).is_err(), "{bad}");
        }
    }
}
