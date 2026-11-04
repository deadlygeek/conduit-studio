//! Evaluates `examples/duplicate` end to end.

mod common;

use common::image;

const BG: [u8; 4] = [0xf4, 0xf1, 0xea, 255];
const TILE: [u8; 4] = [0xe7, 0x6f, 0x51, 255];

#[test]
fn each_copy_lands_in_its_own_place() {
    let values = common::render("duplicate");
    let img = image(&values, "comp");
    assert_eq!(img.dimensions(), (300, 100));
    for (x, y, expected) in [
        (30, 30, TILE),
        (69, 69, TILE),
        (150, 50, TILE),
        (250, 50, TILE),
        (100, 50, BG),
        (200, 50, BG),
        (5, 5, BG),
    ] {
        assert_eq!(img.get_pixel(x, y).0, expected, "pixel ({x},{y})");
    }
}
