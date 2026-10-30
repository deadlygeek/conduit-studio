//! Number params fed from `int` nodes instead of typed values.

use std::path::PathBuf;

use conduit_core::graph::from_sources;
use conduit_core::{Evaluator, NullHost, Reference, Value, Values, evaluate};

fn render(files: &[(&str, &str)]) -> Result<Values, String> {
    let registry = conduit_nodes::registry();
    let sources = files
        .iter()
        .map(|(id, text)| (PathBuf::from(format!("{id}.toml")), text.to_string()));
    let graph = from_sources(sources, &registry).map_err(|e| e.to_string())?;
    evaluate(&graph, &registry, &mut NullHost).map_err(|e| e.to_string())
}

fn value(values: &Values, r: &str) -> Value {
    values[&Reference::parse(r).unwrap()].clone()
}

const SIZE: &str = "type = \"int\"\n[params]\nvalue = 30";
const SQUARE: &str = r#"type = "rect"
[params]
color = "black"
width = 5
[inputs]
width = "size"
height = "size"
"#;
const PLACED: &str = r#"type = "position"
[inputs]
image = "square"
x = "size"
[params]
y = 7
"#;

#[test]
fn int_feeds_dimensions_and_position() {
    let values = render(&[("size", SIZE), ("square", SQUARE), ("placed", PLACED)]).unwrap();
    // The link wins over the typed `width = 5`; `height` has no typed value at all.
    let Value::Image(img) = value(&values, "square") else {
        panic!("expected image")
    };
    assert_eq!(img.dimensions(), (30, 30));
    let Value::Layer(layer) = value(&values, "placed") else {
        panic!("expected layer")
    };
    assert_eq!((layer.x, layer.y), (30, 7), "x linked, y typed");
}

#[test]
fn linked_value_is_range_checked() {
    let zero = "type = \"int\"\n[params]\nvalue = 0";
    let err = render(&[("size", zero), ("square", SQUARE)]).unwrap_err();
    assert!(
        err.contains("`width` from `size.out` must be between 1 and 16384, got 0"),
        "{err}"
    );
}

#[test]
fn only_numbers_can_be_linked() {
    let bad = SQUARE.replace("height = \"size\"", "height = \"other\"");
    let other = "type = \"rect\"\n[params]\ncolor = \"red\"";
    let err = render(&[("size", SIZE), ("other", other), ("square", &bad)]).unwrap_err();
    assert!(
        err.contains("expects number, but `other.out` is image"),
        "{err}"
    );
}

#[test]
fn changing_an_int_reruns_its_consumers_only() {
    let registry = conduit_nodes::registry();
    let mut evaluator = Evaluator::default();
    let mut run = |size: &str| {
        let files = [
            ("size", size.to_string()),
            ("square", SQUARE.to_string()),
            (
                "bg",
                "type = \"rect\"\n[params]\ncolor = \"white\"".to_string(),
            ),
        ];
        let sources = files
            .into_iter()
            .map(|(id, text)| (PathBuf::from(format!("{id}.toml")), text));
        let graph = from_sources(sources, &registry).unwrap();
        evaluator
            .evaluate(&graph, &registry, &mut NullHost)
            .unwrap()
            .1
            .evaluated
    };
    assert_eq!(run(SIZE), 3);
    assert_eq!(run(SIZE), 0);
    assert_eq!(
        run("type = \"int\"\n[params]\nvalue = 31"),
        2,
        "size and square"
    );
}

#[test]
fn color_nodes_feed_color_params() {
    let base = "type = \"color\"\n[params]\ncolor = \"red\"";
    // A second color node takes its base from the first and lightens it.
    let light = "type = \"color\"\n[inputs]\ncolor = \"base\"\n[params]\nlightness = 25";
    // `color` is required on rect, but a link is enough.
    let swatch = "type = \"rect\"\n[inputs]\ncolor = \"light\"\n[params]\nwidth = 2\nheight = 2";
    let values = render(&[("base", base), ("light", light), ("swatch", swatch)]).unwrap();
    let Value::Color(c) = value(&values, "light") else {
        panic!("expected color")
    };
    assert_eq!(c.0, [255, 128, 128, 255]);
    let Value::Image(img) = value(&values, "swatch") else {
        panic!("expected image")
    };
    assert!(img.pixels().all(|p| p.0 == [255, 128, 128, 255]));
}

#[test]
fn colors_and_numbers_do_not_mix() {
    let base = "type = \"color\"\n[params]\ncolor = \"red\"";
    let bad = "type = \"rect\"\n[inputs]\nwidth = \"base\"\n[params]\ncolor = \"white\"";
    let err = render(&[("base", base), ("r", bad)]).unwrap_err();
    assert!(
        err.contains("expects number, but `base.out` is color"),
        "{err}"
    );
}

#[test]
fn palette_colors_feed_color_nodes() {
    let brand = "type = \"palette\"\n[params.colors]\nprimary = \"#264653\"\naccent = \"red\"";
    let dark = "type = \"color\"\n[inputs]\ncolor = \"brand.accent\"\n[params]\nlightness = -25";
    let swatch = "type = \"rect\"\n[inputs]\ncolor = \"dark\"\n[params]\nwidth = 1\nheight = 1";
    let values = render(&[("brand", brand), ("dark", dark), ("swatch", swatch)]).unwrap();
    assert!(
        matches!(value(&values, "brand.primary"), Value::Color(c) if c.0 == [0x26, 0x46, 0x53, 255])
    );
    let Value::Image(img) = value(&values, "swatch") else {
        panic!("expected image")
    };
    assert_eq!(img.get_pixel(0, 0).0, [128, 0, 0, 255]);
}
