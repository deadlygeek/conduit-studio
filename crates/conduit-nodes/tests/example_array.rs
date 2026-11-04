//! Evaluates `examples/array` end to end.

mod common;

use common::image;

const BG: [u8; 4] = [0xf4, 0xf1, 0xea, 255];
const TILE: [u8; 4] = [0xe7, 0x6f, 0x51, 255];
const ODD: [u8; 4] = [0x26, 0x46, 0x53, 255];

#[test]
fn a_grid_with_one_copy_tinted() {
    let values = common::render("array");
    let img = image(&values, "comp");
    assert_eq!(img.dimensions(), (180, 100));
    for (x, y, expected) in [
        (15, 15, TILE),
        (44, 44, TILE),
        (50, 30, BG),
        (60, 20, TILE),
        (150, 20, TILE),
        (164, 84, TILE),
        (20, 60, TILE),
        (60, 60, ODD),
        (84, 84, ODD),
        (90, 60, BG),
        (100, 60, TILE),
        (5, 5, BG),
    ] {
        assert_eq!(img.get_pixel(x, y).0, expected, "pixel ({x},{y})");
    }
}
