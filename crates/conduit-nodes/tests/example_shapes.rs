//! Evaluates `examples/shapes` end to end: a boolean subtract and exclude, rasterized
//! where they meet the composite.

mod common;

use conduit_core::{Reference, Value};

const BG: [u8; 4] = [0xf4, 0xf1, 0xea, 255];
const TEAL: [u8; 4] = [0x2a, 0x9d, 0x8f, 255];
const INK: [u8; 4] = [0x26, 0x46, 0x53, 255];
const ORANGE: [u8; 4] = [0xe7, 0x6f, 0x51, 255];

#[test]
fn subtract_and_exclude_render_as_expected() {
    let values = common::render("shapes");
    assert!(matches!(
        values[&Reference::parse("punched").unwrap()],
        Value::Shape(_)
    ));

    let img = common::image(&values, "comp");
    assert_eq!(img.dimensions(), (400, 240));
    for (x, y, expected, what) in [
        (5, 5, BG, "background"),
        (50, 120, TEAL, "card"),
        (30, 120, INK, "card outline"),
        (110, 120, BG, "hole"),
        (70, 120, INK, "the outline follows the hole"),
        (230, 120, ORANGE, "left circle only"),
        (300, 120, BG, "the overlap is excluded"),
        (370, 120, ORANGE, "right circle only, painted like the left"),
    ] {
        assert_eq!(img.get_pixel(x, y).0, expected, "{what} at ({x},{y})");
    }
}
