//! Evaluates `examples/text` end to end. It uses the machine's own fonts, so the checks
//! are coarse: text in the right colors lands in the right areas.

use std::path::PathBuf;

use conduit_core::{NullHost, Reference, Value, evaluate, load};

const BG: [u8; 4] = [0xf4, 0xf1, 0xea, 255];
const TITLE: [u8; 4] = [0x26, 0x46, 0x53, 255];
const NOTE: [u8; 4] = [0xe7, 0x6f, 0x51, 255];

#[test]
fn title_and_note_are_drawn_where_placed() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/text");
    let registry = conduit_nodes::registry();
    let graph = load(&dir, &registry).unwrap_or_else(|e| panic!("{e}"));
    let values = evaluate(&graph, &registry, &mut NullHost).unwrap();
    let Value::Image(img) = &values[&Reference::parse("comp").unwrap()] else {
        panic!("comp.out should be an image");
    };
    assert_eq!(img.dimensions(), (600, 300));

    let found = |color: [u8; 4], ys: std::ops::Range<u32>| {
        ys.flat_map(|y| (0..600).map(move |x| (x, y)))
            .any(|(x, y)| img.get_pixel(x, y).0 == color)
    };
    // The title's text box starts at (40, 40) and is 64 × 1.2 px tall.
    assert!(found(TITLE, 40..117), "title color in the title's lines");
    assert!(!found(TITLE, 120..300), "no title color below it");
    assert!(found(NOTE, 120..280), "note color below the title");
    assert!(!found(NOTE, 0..100), "no note color up top");
    for (x, y) in [(5, 5), (595, 5), (5, 295), (595, 295)] {
        assert_eq!(img.get_pixel(x, y).0, BG, "corner ({x},{y})");
    }
}
