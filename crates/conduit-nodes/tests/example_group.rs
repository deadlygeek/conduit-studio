//! Evaluates `examples/group` end to end. It uses the machine's own fonts, so the checks
//! are coarse: the word is turned as one piece, with its red letter still in the middle.

mod common;

const BG: [u8; 4] = [0xf4, 0xf1, 0xea, 255];
const INK: [u8; 4] = [0x26, 0x46, 0x53, 255];
const RED: [u8; 4] = [0xe7, 0x6f, 0x51, 255];

#[test]
fn the_word_turns_as_one_piece() {
    let values = common::render("group");
    let img = common::image(&values, "comp");
    assert_eq!(img.dimensions(), (400, 200));

    // The group itself is a whole-pixel layer; the transform after it turns it.
    let group = common::layer(&values, "letters");
    assert!(group.transform.whole_pixel_offset().is_some());
    assert!(
        common::layer(&values, "tilted")
            .transform
            .whole_pixel_offset()
            .is_none()
    );

    // Turned -10° (anticlockwise) about the word's centre: ink rises to the right, so the
    // first letter's ink reaches lower than the last letter's.
    let lowest_ink = |xs: std::ops::Range<u32>| {
        xs.flat_map(|x| (0..200).map(move |y| (x, y)))
            .filter(|&(x, y)| img.get_pixel(x, y).0 == INK)
            .map(|(_, y)| y)
            .max()
            .unwrap()
    };
    assert!(
        lowest_ink(40..90) > lowest_ink(300..360) + 20,
        "the word is tilted"
    );

    // The red letter stays in the middle of the word.
    let red: Vec<u32> = img
        .enumerate_pixels()
        .filter(|(_, _, p)| p.0 == RED)
        .map(|(x, _, _)| x)
        .collect();
    assert!(!red.is_empty());
    let mid = red.iter().sum::<u32>() / red.len() as u32;
    assert!((150..250).contains(&mid), "red letter centred at x = {mid}");

    for (x, y) in [(5, 5), (395, 5), (5, 195), (395, 195)] {
        assert_eq!(img.get_pixel(x, y).0, BG, "corner ({x},{y})");
    }
}
