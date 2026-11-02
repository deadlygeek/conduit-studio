//! Evaluates `examples/transform` end to end.

mod common;

use common::{image, layer};
use conduit_core::{Layer, RgbaImage, Values, raster};

const BG: [u8; 4] = [0xf4, 0xf1, 0xea, 255];
const BAR: [u8; 4] = [0x26, 0x46, 0x53, 255];

fn render() -> Values {
    common::render("transform")
}

fn draw(layer: &Layer) -> RgbaImage {
    let mut canvas = RgbaImage::from_pixel(400, 300, conduit_core::Rgba(BG));
    raster::draw(&mut canvas, layer, (0, 0));
    canvas
}

#[test]
fn transformed_bars_land_where_expected() {
    let values = render();
    let img = image(&values, "comp");
    assert_eq!(img.dimensions(), (400, 300));
    // `turned` rotates about its center, (40 + 60, 40 + 20), so the unturned bar's
    // top-right corner is left empty; `grown` scales about that center after moving 150
    // down.
    for (x, y, expected) in [
        (100, 60, BAR),
        (100, 210, BAR),
        (158, 42, BG),
        (5, 5, BG),
        (399, 299, BG),
    ] {
        assert_eq!(img.get_pixel(x, y).0, expected, "pixel ({x},{y})");
    }
}

#[test]
fn realized_layers_draw_exactly_like_the_original() {
    let values = render();
    let baked = layer(&values, "baked");
    assert!(baked.transform.whole_pixel_offset().is_some());
    assert_eq!(draw(&baked), draw(&layer(&values, "grown")));
}

#[test]
fn nearest_filter_has_hard_edges() {
    let canvas = draw(&layer(&render(), "slanted"));
    assert!(
        canvas.pixels().all(|p| p.0 == BG || p.0 == BAR),
        "nearest should only produce the bar's or the canvas's color"
    );
}
