//! Evaluates `examples/canvas` end to end: a canvas filled from colour and size, and a
//! canvas made from a connected image at half opacity.

mod common;

#[test]
fn filled_canvas_and_faded_image_canvas() {
    let values = common::render("canvas");

    let bg = common::image(&values, "bg");
    assert_eq!(bg.dimensions(), (40, 30));
    assert!(bg.pixels().all(|p| p.0 == [0, 0, 128, 255]));

    let faded = common::image(&values, "faded");
    assert_eq!(faded.dimensions(), (40, 30), "the image decides the size");
    assert_eq!(faded.get_pixel(0, 0).0, [0, 0, 128, 128]);
    assert_eq!(faded.get_pixel(5, 5).0, [255, 255, 255, 128]);
    assert_eq!(faded.get_pixel(15, 15).0, [0, 0, 128, 128]);
}
