//! The `text` node: text set in an installed font, or a font file linked from a `font`
//! node, as a layer; and `split_text`, the same text with each character a layer of its
//! own.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use conduit_core::{
    Affine, Color, DetachPlan, DetachedNode, DetachedSource, EvalCtx, FontData, Inputs, Layer,
    MAX_REPEAT, Node, NodeError, NodeResult, NodeSpec, Outputs, ParamSpec, PortSpec, Rgba,
    RgbaImage, Value, ValueType, raster,
};
use cosmic_text::fontdb::{self, Database, Source};
use cosmic_text::{
    Attrs, Buffer, Family, FeatureTag, FontFeatures, FontSystem, Metrics, Shaping, SwashCache,
    Weight,
};
use serde::Deserialize;

use crate::MAX_DIMENSION;

/// Line spacing as a multiple of the font size.
const LINE_HEIGHT: f32 = 1.2;
const MAX_SIZE: f64 = 2048.0;

/// Renders text with an installed font, or with the font linked into its `font` input
/// (from a `font` node). The layer's origin is the top-left of the text
/// box (the lines' boxes, as in CSS), so a `transform` positions and pivots it
/// predictably; glyphs that poke outside the box (italic overhangs, accents) are kept.
pub struct Text {
    fonts: SharedFonts,
}

/// Loaded fonts, shared by a `text` node and the `split_text` made from it (see
/// [`Text::splitter`]) so installed fonts are only loaded once.
type SharedFonts = Arc<OnceLock<Mutex<Fonts>>>;

struct Fonts {
    system: FontSystem,
    cache: SwashCache,
    /// Font files from `font` nodes already added to `system`, by content hash → the
    /// family name they were added under.
    linked: HashMap<u64, String>,
}

impl Text {
    /// Uses the fonts installed on this machine, loaded on first use.
    pub fn system() -> Self {
        Self {
            fonts: Arc::new(OnceLock::new()),
        }
    }

    /// A `split_text` node using the same fonts as this one.
    pub fn splitter(&self) -> SplitText {
        SplitText {
            fonts: self.fonts.clone(),
        }
    }

    /// Uses only the fonts in `db` (e.g. fonts bundled with tests).
    pub fn with_fonts(db: Database) -> Self {
        let text = Self::system();
        let _ = text.fonts.set(Mutex::new(Fonts::new(db)));
        text
    }

    #[cfg(test)]
    fn fonts(&self) -> &Mutex<Fonts> {
        fonts(&self.fonts)
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
            linked: HashMap::new(),
        }
    }

    /// Adds a linked font file (once per distinct contents) under a family name of its
    /// own, so it can't be confused with an installed family of the same name, and returns
    /// that name. Installed fonts still fill in characters the file lacks.
    fn linked_family(&mut self, font: &FontData) -> NodeResult<String> {
        if let Some(name) = self.linked.get(&font.hash) {
            return Ok(name.clone());
        }
        let name = format!("conduit linked font {:016x}", font.hash);
        let db = self.system.db_mut();
        let ids = db.load_font_source(Source::Binary(font.data.clone()));
        if ids.is_empty() {
            return Err(NodeError::new(format!(
                "`{}` is not a TrueType or OpenType font",
                font.name
            )));
        }
        for id in ids {
            let Some(mut face) = db.face(id).cloned() else {
                continue;
            };
            db.remove_face(id);
            face.families = vec![(name.clone(), fontdb::Language::English_UnitedStates)];
            db.push_face_info(face);
        }
        self.linked.insert(font.hash, name.clone());
        Ok(name)
    }
}

fn fonts(shared: &SharedFonts) -> &Mutex<Fonts> {
    shared.get_or_init(|| Mutex::new(Fonts::new(system_fonts())))
}

/// Locks `shared` and adds the font linked into `inputs`, if any; returns the fonts and the
/// linked family's name (which replaces the `font` param).
fn lock_with_link<'a>(
    shared: &'a SharedFonts,
    inputs: &Inputs,
) -> NodeResult<(std::sync::MutexGuard<'a, Fonts>, Option<String>)> {
    let mut fonts = fonts(shared).lock().unwrap_or_else(|e| e.into_inner());
    let linked = match inputs.get("font") {
        Some(Value::Font(font)) => Some(fonts.linked_family(font)?),
        _ => None,
    };
    Ok((fonts, linked))
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
    font: String,
    size: f64,
    color: Color,
    weight: u16,
    style: Style,
}

/// The params `text` and `split_text` share, after `text` itself.
fn style_params() -> Vec<ParamSpec> {
    vec![
                ParamSpec::font("font", "\"sans-serif\"")
                    .default("\"sans-serif\"")
                    .doc("an installed family (see `conduit fonts`), serif / sans-serif / monospace, or linked from a `font` node"),
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
    ]
}

/// `split_text`'s params: `text`'s, plus where the text box goes (one `transform` can't
/// move every letter at once).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SplitParams {
    text: String,
    font: String,
    size: f64,
    color: Color,
    weight: u16,
    style: Style,
    x: i64,
    y: i64,
}

impl SplitParams {
    fn parse(params: &toml::Table) -> NodeResult<(Params, (i64, i64))> {
        let p: SplitParams = SplitText::spec().parse_params(params)?;
        let n = p.text.chars().filter(|c| !c.is_whitespace()).count();
        if n > MAX_REPEAT {
            return Err(NodeError::new(format!(
                "`text` has {n} characters to split, more than {MAX_REPEAT}"
            )));
        }
        let style = Params {
            text: p.text,
            font: p.font,
            size: p.size,
            color: p.color,
            weight: p.weight,
            style: p.style,
        };
        Ok((style, (p.x, p.y)))
    }
}

impl Node for Text {
    fn spec(&self) -> NodeSpec {
        let mut params =
            vec![ParamSpec::text("text", "\"Hello\"").doc("the text; \\n starts a new line")];
        params.extend(style_params());
        NodeSpec {
            kind: "text",
            inputs: vec![],
            outputs: vec![PortSpec::new("out", ValueType::Layer)],
            params,
            gui: false,
        }
    }

    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        self.spec().parse_params::<Params>(params).map(|_| ())
    }

    fn eval(&self, inputs: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let p: Params = self.spec().parse_params(params)?;
        let (mut fonts, linked) = lock_with_link(&self.fonts, inputs)?;
        let layer = render(&mut fonts, &p, linked)?;
        Ok(Outputs::out(Value::Layer(layer)))
    }
}

/// Lays out `p.text` in `linked` (a family added by [`Fonts::linked_family`]) if given, else
/// in `p.font`. `None` for empty text. With `separate`, ligatures are off so every
/// character gets a glyph of its own.
fn shape(
    system: &mut FontSystem,
    p: &Params,
    linked: Option<String>,
    separate: bool,
) -> NodeResult<Option<Buffer>> {
    let generic = p.font.to_ascii_lowercase();
    let named = match linked {
        Some(linked) => Some(linked),
        None if GENERIC_FAMILIES.contains(&generic.as_str()) => None,
        None => Some(find_family(system.db(), &p.font).ok_or_else(|| {
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
        return Ok(None);
    }

    let size = p.size as f32;
    let mut buffer = Buffer::new(system, Metrics::new(size, size * LINE_HEIGHT));
    let mut attrs = Attrs::new()
        .family(family)
        .weight(Weight(p.weight))
        .style(match p.style {
            Style::Normal => fontdb::Style::Normal,
            Style::Italic => fontdb::Style::Italic,
        });
    if separate {
        let mut features = FontFeatures::new();
        features
            .disable(FeatureTag::new(b"liga"))
            .disable(FeatureTag::new(b"clig"))
            .disable(FeatureTag::new(b"dlig"));
        attrs = attrs.font_features(features);
    }
    buffer.set_size(None, None);
    buffer.set_text(&p.text, &attrs, Shaping::Advanced, None);
    buffer.shape_until_scroll(system, false);
    Ok(Some(buffer))
}

/// Sets `p.text` (see [`shape`]) as one layer.
fn render(fonts: &mut Fonts, p: &Params, linked: Option<String>) -> NodeResult<Layer> {
    let Fonts { system, cache, .. } = fonts;
    let Some(mut buffer) = shape(system, p, linked, false)? else {
        return Ok(Layer::new(Arc::new(RgbaImage::new(0, 0))));
    };

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

    to_layer(pixels, Some((box_w.ceil() as i64, box_h.ceil() as i64)))
}

/// Draws glyph pixels (in text-box coordinates) into a layer placed where they sit in the
/// box. With `text_box` (its size), the image covers the box plus any ink outside it;
/// without, just the ink.
fn to_layer(
    pixels: Vec<(i32, i32, cosmic_text::Color)>,
    text_box: Option<(i64, i64)>,
) -> NodeResult<Layer> {
    let (mut x0, mut y0, mut x1, mut y1) = match text_box {
        Some((w, h)) => (0, 0, w, h),
        None => (i64::MAX, i64::MAX, i64::MIN, i64::MIN),
    };
    if pixels.is_empty() && text_box.is_none() {
        return Ok(Layer::new(Arc::new(RgbaImage::new(0, 0))));
    }
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

/// Sets text like `text`, but each character (whitespace aside) is a layer of its own,
/// `char1`, `char2`, … in reading order, drawn just where it sits in the text, so
/// compositing them all gives the whole text. Ligatures are off, so "fi" is two layers.
pub struct SplitText {
    fonts: SharedFonts,
}

impl SplitText {
    fn spec() -> NodeSpec {
        let mut params = vec![
            ParamSpec::text("text", "\"Hello\"")
                .fixed()
                .doc("the text; each character except spaces and line breaks is an output"),
        ];
        params.extend(style_params());
        params.extend([
            ParamSpec::int(
                "x",
                -i64::from(MAX_DIMENSION),
                i64::from(MAX_DIMENSION),
                "0",
            )
            .default("0")
            .doc("move right: where the text box's left edge lands, in parent pixels"),
            ParamSpec::int(
                "y",
                -i64::from(MAX_DIMENSION),
                i64::from(MAX_DIMENSION),
                "0",
            )
            .default("0")
            .doc("move down: where the text box's top edge lands, in parent pixels"),
        ]);
        NodeSpec {
            kind: "split_text",
            inputs: vec![],
            outputs: vec![
                PortSpec::new("char", ValueType::Layer)
                    .repeated(&["text"])
                    .grouped(),
            ],
            params,
            gui: false,
        }
    }
}

impl Node for SplitText {
    fn spec(&self) -> NodeSpec {
        SplitText::spec()
    }

    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        SplitParams::parse(params).map(|_| ())
    }

    fn eval(&self, inputs: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let (p, (x, y)) = SplitParams::parse(params)?;
        let (mut fonts, linked) = lock_with_link(&self.fonts, inputs)?;
        let values = split(&mut fonts, &p, linked)?
            .into_iter()
            .enumerate()
            .map(|(i, Letter { mut layer, .. })| {
                layer.transform = Affine::translate(x as f64, y as f64) * layer.transform;
                (format!("char{}", i + 1), Value::Layer(layer))
            })
            .collect();
        Ok(Outputs { values })
    }

    fn can_detach(&self) -> bool {
        true
    }

    /// Each letter becomes a one-character `text` node and a `transform` putting it where
    /// it sat in the text. Kerning is kept as it was; a later font or size change won't
    /// re-space the letters.
    fn detach(
        &self,
        inputs: &Inputs,
        params: &toml::Table,
        ctx: &mut EvalCtx,
    ) -> Option<NodeResult<DetachPlan>> {
        let plan = || {
            let (p, (x, y)) = SplitParams::parse(params)?;
            let (mut fonts, linked) = lock_with_link(&self.fonts, inputs)?;
            let letters = split(&mut fonts, &p, linked)?;
            let chars = p.text.chars().filter(|c| !c.is_whitespace());
            let (mut nodes, mut outputs) = (Vec::new(), Vec::new());
            for (i, (letter, c)) in letters.iter().zip(chars).enumerate() {
                let out = format!("char{}", i + 1);
                let text_name = format!("{out}_text");
                // The style params as written, except those fed by an input, which stay
                // linked to the same source.
                let mut style: toml::Table = params
                    .iter()
                    .filter(|(k, _)| !["text", "x", "y"].contains(&k.as_str()))
                    .filter(|(k, _)| inputs.get(k).is_none())
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                style.insert("text".into(), toml::Value::String(c.to_string()));
                let links = ["font", "size", "color", "weight"]
                    .into_iter()
                    .filter(|port| inputs.get(port).is_some())
                    .map(|port| (port.to_string(), DetachedSource::Input(port.to_string())))
                    .collect();
                nodes.push(DetachedNode {
                    name: text_name.clone(),
                    comment: format!("\"{c}\", letter {} of {}, detached", i + 1, ctx.node_id),
                    kind: "text",
                    inputs: links,
                    params: style,
                });
                let (dx, dy) = letter.pen;
                let mut at = toml::Table::new();
                at.insert("x".into(), toml::Value::Integer(x + dx));
                at.insert("y".into(), toml::Value::Integer(y + dy));
                nodes.push(DetachedNode {
                    name: out.clone(),
                    comment: format!("\"{c}\" placed where it sat in {}", ctx.node_id),
                    kind: "transform",
                    inputs: vec![("layer".into(), DetachedSource::Node(text_name))],
                    params: at,
                });
                outputs.push((out.clone(), out));
            }
            Ok(DetachPlan { nodes, outputs })
        };
        Some(plan())
    }
}

/// One character of a split text.
struct Letter {
    /// Its ink, placed where it sits in the text box.
    layer: Layer,
    /// Where the same character set alone (by `text`) must move to land in its place:
    /// its pen position in the text, in whole pixels.
    pen: (i64, i64),
}

/// One layer per non-whitespace character of `p.text` (see [`SplitText`]). A character
/// with no glyph of its own (one the font joins into its neighbour's) is an empty layer.
fn split(fonts: &mut Fonts, p: &Params, linked: Option<String>) -> NodeResult<Vec<Letter>> {
    let Fonts { system, cache, .. } = fonts;
    let Some(buffer) = shape(system, p, linked, true)? else {
        return Ok(Vec::new());
    };

    // Each line's characters, numbered across the whole text skipping whitespace: the
    // character starting at byte `b` of line `l` is `ordinals[l][&b]`.
    let mut ordinals: Vec<HashMap<usize, usize>> = Vec::new();
    let mut next = 0;
    for line in &buffer.lines {
        let mut map = HashMap::new();
        for (b, c) in line.text().char_indices() {
            if !c.is_whitespace() {
                map.insert(b, next);
                next += 1;
            }
        }
        ordinals.push(map);
    }

    let [r, g, b, a] = p.color.0.0;
    let color = cosmic_text::Color::rgba(r, g, b, a);
    let mut pixels: Vec<Vec<(i32, i32, cosmic_text::Color)>> = vec![Vec::new(); next];
    let mut pens: Vec<Option<(i64, i64)>> = vec![None; next];
    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            let Some(&i) = ordinals[run.line_i].get(&glyph.start) else {
                continue; // whitespace
            };
            // Set alone, the character's glyph would start at x = 0 on the first line.
            pens[i].get_or_insert_with(|| {
                let alone = cosmic_text::LayoutGlyph {
                    x: 0.0,
                    ..glyph.clone()
                };
                (
                    i64::from(physical_x(glyph) - physical_x(&alone)),
                    run.line_top.round() as i64,
                )
            });
            let physical = glyph.physical((0., run.line_y), 1.0);
            let glyph_color = glyph.color_opt.unwrap_or(color);
            cache.with_pixels(system, physical.cache_key, glyph_color, |x, y, c| {
                if c.a() > 0 {
                    pixels[i].push((physical.x + x, physical.y + y, c));
                }
            });
        }
    }
    // Each letter's image is just its ink, so a transform pivots about the letter.
    pixels
        .into_iter()
        .zip(pens)
        .map(|(px, pen)| {
            Ok(Letter {
                layer: to_layer(px, None)?,
                pen: pen.unwrap_or((0, 0)),
            })
        })
        .collect()
}

/// A glyph's x as drawn, in whole pixels.
fn physical_x(glyph: &cosmic_text::LayoutGlyph) -> i32 {
    glyph.physical((0., 0.), 1.0).x
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::{params, run_layer};

    fn fixture_fonts() -> Text {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fonts");
        let mut db = Database::new();
        db.load_fonts_dir(dir);
        assert!(db.len() >= 2, "bundled fonts missing from {dir}");
        Text::with_fonts(db)
    }

    fn text(node: &Text, toml: &str) -> NodeResult<Layer> {
        run_layer(node, vec![], &params(toml))
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

    fn droid(file: &str) -> Value {
        let path = format!("{}/tests/fixtures/fonts/{file}", env!("CARGO_MANIFEST_DIR"));
        Value::Font(FontData::new(file, std::fs::read(path).unwrap()))
    }

    fn linked(node: &Text, font: Value, toml: &str) -> NodeResult<Layer> {
        run_layer(node, vec![("font", vec![font])], &params(toml))
    }

    #[test]
    fn a_linked_font_replaces_the_font_param() {
        // No installed fonts at all: only the linked file can draw anything.
        let node = Text::with_fonts(Database::new());
        let layer = linked(&node, droid("DroidSans.ttf"), "text = \"Hello\"").unwrap();
        assert!(ink(&layer) > 100);
        // The param is ignored, even when it names a family that isn't installed.
        let again = linked(
            &node,
            droid("DroidSans.ttf"),
            "text = \"Hello\"\nfont = \"Nope Sans\"",
        )
        .unwrap();
        assert_eq!(again.image.as_raw(), layer.image.as_raw());
        // Each distinct file is added once.
        let faces = || node.fonts().lock().unwrap().system.db().len();
        assert_eq!(faces(), 1);
        linked(&node, droid("DroidSans-Bold.ttf"), "text = \"Hello\"").unwrap();
        assert_eq!(faces(), 2);
    }

    #[test]
    fn a_linked_font_is_not_confused_with_an_installed_family() {
        // "Droid Sans" is installed (regular and bold); the linked bold file is used as is,
        // at weight 400, rather than looked up by its family name.
        let node = fixture_fonts();
        let bold = text(&node, "text = \"Hello\"\nweight = 700").unwrap();
        let regular = text(&node, "text = \"Hello\"").unwrap();
        let link = linked(&node, droid("DroidSans-Bold.ttf"), "text = \"Hello\"").unwrap();
        assert_eq!(link.image.as_raw(), bold.image.as_raw());
        assert_ne!(link.image.as_raw(), regular.image.as_raw());
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

    fn split_text(node: &Text, toml: &str) -> NodeResult<Outputs> {
        let split = node.splitter();
        let inputs = Inputs::new(HashMap::new());
        let mut ctx = EvalCtx {
            node_id: "split",
            root: std::path::Path::new(""),
            gui: &mut conduit_core::NullHost,
        };
        split.eval(&inputs, &params(toml), &mut ctx)
    }

    fn letter(out: &Outputs, name: &str) -> Layer {
        match &out.values[name] {
            Value::Layer(l) => l.clone(),
            _ => panic!("{name}: expected a layer"),
        }
    }

    /// Positions in the parent of every inked pixel of `layer` (moved by whole pixels).
    fn inked(layer: &Layer) -> std::collections::BTreeSet<(i64, i64)> {
        let (ox, oy) = layer.transform.whole_pixel_offset().unwrap();
        layer
            .image
            .enumerate_pixels()
            .filter(|(_, _, p)| p[3] > 0)
            .map(|(x, y, _)| (ox + i64::from(x), oy + i64::from(y)))
            .collect()
    }

    #[test]
    fn split_letters_sit_where_the_text_draws_them() {
        let node = fixture_fonts();
        let out = split_text(&node, "text = \"Hello\"\nsize = 40").unwrap();
        assert_eq!(out.values.len(), 5);
        let whole = text(&node, "text = \"Hello\"\nsize = 40").unwrap();
        let mut together = std::collections::BTreeSet::new();
        let mut last_x = i64::MIN;
        for i in 1..=5 {
            let l = letter(&out, &format!("char{i}"));
            assert!(ink(&l) > 0, "char{i} has ink");
            let (x, _) = l.transform.whole_pixel_offset().unwrap();
            assert!(x > last_x, "char{i} is right of the one before");
            last_x = x;
            // Each image is just the letter's ink: no empty border.
            let (w, h) = l.image.dimensions();
            let edge = |x: u32, y: u32| l.image.get_pixel(x, y)[3] > 0;
            assert!((0..h).any(|y| edge(0, y)) && (0..h).any(|y| edge(w - 1, y)));
            assert!((0..w).any(|x| edge(x, 0)) && (0..w).any(|x| edge(x, h - 1)));
            together.extend(inked(&l));
        }
        assert_eq!(
            together,
            inked(&whole),
            "the letters cover exactly the text's ink"
        );
    }

    #[test]
    fn split_skips_whitespace_and_keeps_ligatures_apart() {
        let node = fixture_fonts();
        let out = split_text(&node, "text = \"a b\\nfi\"").unwrap();
        let names: std::collections::BTreeSet<_> = out.values.keys().cloned().collect();
        assert_eq!(
            names,
            ["char1", "char2", "char3", "char4"]
                .map(String::from)
                .into()
        );
        for i in 1..=4 {
            assert!(
                ink(&letter(&out, &format!("char{i}"))) > 0,
                "char{i} has ink"
            );
        }
        let at = |i: usize| {
            letter(&out, &format!("char{i}"))
                .transform
                .whole_pixel_offset()
                .unwrap()
        };
        assert!(at(3).1 > at(1).1 + 20, "the second line is lower");
        assert!(at(4).0 > at(3).0, "f and i are separate letters");
        let spec = node.splitter().spec().with_param_inputs();
        assert_eq!(spec.outputs_for(&params("text = \"a b\\nfi\"")).len(), 4);
        assert!(spec.input("text").is_none() && spec.input("color").is_some());
    }

    #[test]
    fn split_moves_every_letter_by_x_and_y() {
        let node = fixture_fonts();
        let here = split_text(&node, "text = \"Hi\"").unwrap();
        let there = split_text(&node, "text = \"Hi\"\nx = 40\ny = -7").unwrap();
        for name in ["char1", "char2"] {
            let (ax, ay) = letter(&here, name).transform.whole_pixel_offset().unwrap();
            let (bx, by) = letter(&there, name).transform.whole_pixel_offset().unwrap();
            assert_eq!((bx - ax, by - ay), (40, -7), "{name}");
        }
    }

    #[test]
    fn split_rejects_too_many_letters() {
        let node = fixture_fonts().splitter();
        let long = format!("text = \"{}\"", "x".repeat(MAX_REPEAT + 1));
        assert!(node.check_params(&params(&long)).is_err());
        let spaced = format!("text = \"{}\"", "x ".repeat(MAX_REPEAT));
        assert!(
            node.check_params(&params(&spaced)).is_ok(),
            "spaces don't count"
        );
    }
}
