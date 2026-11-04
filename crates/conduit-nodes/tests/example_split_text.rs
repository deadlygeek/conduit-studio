//! Evaluates `examples/split_text` end to end. It uses the machine's own fonts, so the
//! checks are coarse: each letter's colour lands in its own part of the word.

mod common;

const BG: [u8; 4] = [0xf4, 0xf1, 0xea, 255];
const INK: [u8; 4] = [0x26, 0x46, 0x53, 255];
const RED: [u8; 4] = [0xe7, 0x6f, 0x51, 255];

#[test]
fn one_letter_recoloured_and_one_turned() {
    let values = common::render("split_text");
    let img = common::image(&values, "comp");
    assert_eq!(img.dimensions(), (400, 160));

    let h1 = letter_span(&values, "word.char1");
    let l3 = letter_span(&values, "word.char3");
    let l4 = letter_span(&values, "word.char4");
    let found = |color: [u8; 4], xs: std::ops::Range<i64>| {
        xs.flat_map(|x| (0..160).map(move |y| (x, y)))
            .any(|(x, y)| img.get_pixel(x as u32, y as u32).0 == color)
    };
    assert!(found(RED, l3.clone()), "the third letter is red");
    assert!(!found(RED, h1.start..l3.start), "nothing before it is");
    assert!(!found(RED, l3.end..400), "nothing after it is");
    assert!(
        found(INK, h1.clone()) && found(INK, l4),
        "the others keep their colour"
    );
    for (x, y) in [(5, 5), (395, 5), (5, 155), (395, 155)] {
        assert_eq!(img.get_pixel(x, y).0, BG, "corner ({x},{y})");
    }

    // The O is turned: its layer is no longer a plain move.
    let o = common::layer(&values, "tilted_o");
    assert!(o.transform.whole_pixel_offset().is_none());
}

/// The x range a letter's layer covers.
fn letter_span(values: &conduit_core::Values, id: &str) -> std::ops::Range<i64> {
    let l = common::layer(values, id);
    let (x, _) = l.transform.whole_pixel_offset().unwrap();
    x..x + i64::from(l.image.width())
}
