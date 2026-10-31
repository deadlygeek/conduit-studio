//! Evaluates `examples/effects` end to end.

use std::path::PathBuf;

use conduit_core::{Layer, NullHost, Reference, Rgba, Value, Values, adjust, evaluate, load};

const ACCENT: [u8; 4] = [0xe7, 0x6f, 0x51, 255];
const TEAL: [u8; 4] = [0x2a, 0x9d, 0x8f, 255];
const PRIMARY: [u8; 4] = [0x26, 0x46, 0x53, 255];

fn render() -> Values {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/effects");
    let registry = conduit_nodes::registry();
    let graph = load(&dir, &registry).unwrap_or_else(|e| panic!("{e}"));
    evaluate(&graph, &registry, &mut NullHost).unwrap()
}

fn layer(values: &Values, id: &str) -> Layer {
    match &values[&Reference::parse(id).unwrap()] {
        Value::Layer(l) => l.clone(),
        other => panic!("{id}: expected layer, got {other:?}"),
    }
}

#[test]
fn tint_recolors_black_text_to_the_palette_accent() {
    let logo = layer(&render(), "logo_tinted");
    let mut drawn = 0;
    for px in logo.image.pixels().filter(|p| p[3] > 0) {
        // Anti-aliasing only varies alpha, so every visible pixel is exactly the accent.
        assert_eq!(&px.0[..3], &ACCENT[..3]);
        drawn += 1;
    }
    assert!(drawn > 0, "the logo has visible pixels");
}

#[test]
fn hsl_and_invert_land_on_the_canvas() {
    let values = render();
    let Value::Image(img) = &values[&Reference::parse("comp").unwrap()] else {
        panic!("comp.out should be an image");
    };
    assert_eq!(img.dimensions(), (400, 300));

    let shifted = adjust(Rgba(TEAL), 180.0, 0.0, -10.0, 100.0);
    assert_eq!(*img.get_pixel(80, 220), shifted, "swatch center");

    // The turned bar's center stays at (220 + 60, 200 + 20); `invert` realized it there.
    let [r, g, b, a] = PRIMARY;
    assert_eq!(img.get_pixel(280, 220).0, [255 - r, 255 - g, 255 - b, a]);
    assert!(
        layer(&values, "bar_inverted")
            .transform
            .whole_pixel_offset()
            .is_some()
    );
}
