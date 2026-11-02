//! Evaluates `examples/effects` end to end.

mod common;

use common::{image, layer};
use conduit_core::{Rgba, Values, adjust};

const ACCENT: [u8; 4] = [0xe7, 0x6f, 0x51, 255];
const TEAL: [u8; 4] = [0x2a, 0x9d, 0x8f, 255];
const PRIMARY: [u8; 4] = [0x26, 0x46, 0x53, 255];

fn render() -> Values {
    common::render("effects")
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
    let img = image(&values, "comp");
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
