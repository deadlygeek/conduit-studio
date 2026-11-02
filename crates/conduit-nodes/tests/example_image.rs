//! Evaluates `examples/image` end to end: a JPEG and a transparent PNG read from the
//! project's `assets/` folder.

mod common;

use conduit_core::{NullHost, evaluate};

#[test]
fn badge_over_photo() {
    let (dir, registry, graph) = common::load_example("image");
    assert_eq!(
        graph.files(&registry),
        [
            dir.join("assets/images/badge.png"),
            dir.join("assets/images/sunset.jpg")
        ]
    );
    let values = evaluate(&graph, &registry, &mut NullHost).unwrap();
    let img = common::image(&values, "comp");
    assert_eq!(img.dimensions(), (480, 320));

    let near = |(x, y): (u32, u32), want: [u8; 3]| {
        let px = img.get_pixel(x, y).0;
        assert_eq!(px[3], 255, "({x},{y}) opaque");
        let off = (0..3).map(|i| px[i].abs_diff(want[i])).max().unwrap();
        assert!(off <= 12, "({x},{y}) is {px:?}, expected about {want:?}");
    };
    near((340, 200), [255, 214, 120]); // the sun
    near((240, 300), [30, 40, 60]); // the ground
    // The badge is scaled about its center, so it covers 50..110 with its center at 80.
    near((80, 80), [231, 111, 81]);
    // The photo's sky shows through the badge's transparent corners.
    near((51, 51), [72, 73, 130]);
}
