//! Detaching `array` and `split_text` in the examples: the picture stays the same, the
//! generator is gone, and its users now take the new nodes.

mod common;

use std::path::Path;

use conduit_core::{NullHost, RgbaImage, detach_plan, edit, evaluate, inspect, load};

/// Copies `examples/<name>` into a temporary folder.
fn copy_example(name: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().unwrap();
    for entry in std::fs::read_dir(common::example_dir(name)).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, tmp.path().join(path.file_name().unwrap())).unwrap();
    }
    tmp
}

fn render(dir: &Path) -> RgbaImage {
    let registry = conduit_nodes::registry();
    let graph = load(dir, &registry).unwrap_or_else(|e| panic!("{e}"));
    let values = evaluate(&graph, &registry, &mut NullHost).unwrap();
    common::image(&values, "comp").clone()
}

fn detach(dir: &Path, id: &str) -> edit::Change {
    let registry = conduit_nodes::registry();
    let graph = load(dir, &registry).unwrap();
    let plan = detach_plan(&graph, &registry, id).unwrap();
    let project = inspect(dir, &registry).unwrap();
    edit::detach_node(dir, &project, id, &plan).unwrap()
}

fn read(dir: &Path, id: &str) -> String {
    std::fs::read_to_string(dir.join(format!("{id}.toml"))).unwrap()
}

#[test]
fn a_detached_array_draws_the_same() {
    let tmp = copy_example("array");
    let dir = tmp.path();
    let before = render(dir);
    let change = detach(dir, "grid");

    assert!(!dir.join("grid.toml").exists(), "the array is gone");
    assert_eq!(
        read(dir, "grid_out6"),
        "# Copy 6 (column 2, row 2) of grid's array, detached\n\
         type = \"transform\"\n\n[inputs]\nlayer = \"place\"\n\n[params]\nx = 40\ny = 40\n"
    );
    assert!(
        read(dir, "odd").contains("layer = \"grid_out6\""),
        "the tint follows its copy"
    );
    assert!(read(dir, "comp").contains("\"grid_out1\""));
    assert_eq!(render(dir).as_raw(), before.as_raw(), "pixel for pixel");

    change.undo().unwrap();
    assert!(dir.join("grid.toml").exists() && !dir.join("grid_out1.toml").exists());
}

#[test]
fn a_detached_turning_array_draws_the_same() {
    let tmp = copy_example("array");
    let dir = tmp.path();
    let grid = read(dir, "grid").replace(
        "gap_y = 10",
        "gap_y = 10\nrotate_step = 15\nscale_step = 0.9",
    );
    std::fs::write(dir.join("grid.toml"), grid).unwrap();
    let before = render(dir);
    detach(dir, "grid");
    let after = render(dir);
    // The same matrices, up to the params' rounding to 6 decimals.
    let diff = before
        .pixels()
        .zip(after.pixels())
        .flat_map(|(a, b)| a.0.into_iter().zip(b.0).map(|(a, b)| a.abs_diff(b)))
        .max()
        .unwrap();
    assert!(diff <= 1, "max channel difference {diff}");
    assert!(read(dir, "grid_out3").contains("rotate = 30"));
}

#[test]
fn detached_letters_land_where_they_were() {
    let tmp = copy_example("split_text");
    let dir = tmp.path();
    let before = render(dir);
    detach(dir, "word");

    assert!(!dir.join("word.toml").exists());
    let l = read(dir, "word_char3_text");
    assert!(
        l.starts_with("# \"L\", letter 3 of word, detached\ntype = \"text\""),
        "{l}"
    );
    assert!(l.contains("text = \"L\"") && l.contains("weight = 700") && !l.contains("x ="));
    assert!(read(dir, "red_l").contains("layer = \"word_char3\""));
    assert!(read(dir, "word_char3").contains("layer = \"word_char3_text\""));

    // Each letter is drawn alone now, so anti-aliased edges can differ a little (the glyph
    // no longer sits at a fraction of a pixel); the letters themselves don't move.
    let after = render(dir);
    let ink = |img: &RgbaImage| -> std::collections::HashSet<(u32, u32)> {
        img.enumerate_pixels()
            .filter(|(_, _, p)| p.0 != [0xf4, 0xf1, 0xea, 255])
            .map(|(x, y, _)| (x, y))
            .collect()
    };
    let (a, b) = (ink(&before), ink(&after));
    let same = a.intersection(&b).count();
    assert!(
        same as f64 > 0.98 * a.len() as f64 && b.len().abs_diff(a.len()) < a.len() / 50,
        "{same} of {} inked pixels kept; {} after",
        a.len(),
        b.len()
    );
}

#[test]
fn other_nodes_cant_be_detached() {
    let tmp = copy_example("array");
    let registry = conduit_nodes::registry();
    let graph = load(tmp.path(), &registry).unwrap();
    let err = detach_plan(&graph, &registry, "place").unwrap_err();
    assert_eq!(
        err.to_string(),
        "place: `transform` nodes can't be detached"
    );
}
