//! Evaluates `examples/basic` end to end and checks the rendered pixels.

mod common;

#[test]
fn black_square_on_white_canvas() {
    let values = common::render("basic");
    let img = common::image(&values, "comp");
    assert_eq!(img.dimensions(), (800, 600));

    const WHITE: [u8; 4] = [255, 255, 255, 255];
    const BLACK: [u8; 4] = [0, 0, 0, 255];
    for (x, y, expected) in [
        (10, 10, WHITE),
        (299, 299, WHITE),
        (300, 300, BLACK),
        (349, 349, BLACK),
        (350, 350, WHITE),
        (799, 599, WHITE),
    ] {
        assert_eq!(img.get_pixel(x, y).0, expected, "pixel ({x},{y})");
    }
    let black = img.pixels().filter(|p| p.0 == BLACK).count();
    assert_eq!(black, 50 * 50);
}
