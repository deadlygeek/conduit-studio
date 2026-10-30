//! Evaluates `examples/basic` end to end and checks the rendered pixels.

use std::path::PathBuf;

use conduit_core::{NullHost, Reference, Value, evaluate, load};

#[test]
fn black_square_on_white_canvas() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/basic");
    let registry = conduit_nodes::registry();
    let graph = load(&dir, &registry).unwrap_or_else(|e| panic!("{e}"));
    let values = evaluate(&graph, &registry, &mut NullHost).unwrap();

    let Value::Image(img) = &values[&Reference::parse("comp").unwrap()] else {
        panic!("comp.out should be an image");
    };
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
