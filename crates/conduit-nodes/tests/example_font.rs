//! Evaluates `examples/font` end to end: text set in a font file kept in the project.

use std::path::PathBuf;

use conduit_core::{EvalCtx, Inputs, Node, NullHost, Reference, Value, evaluate, load};
use conduit_nodes::Text;
use cosmic_text::fontdb::Database;

#[test]
fn title_uses_the_project_font() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/font");
    let registry = conduit_nodes::registry();
    let graph = load(&dir, &registry).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        graph.files(&registry),
        [dir.join("assets/fonts/DroidSans.ttf")]
    );
    let values = evaluate(&graph, &registry, &mut NullHost).unwrap();
    let Value::Layer(title) = &values[&Reference::parse("title").unwrap()] else {
        panic!("title.out should be a layer");
    };

    // The same text with Droid Sans as the only installed font, picked by name: identical,
    // whatever fonts this machine has.
    let mut db = Database::new();
    db.load_font_file(dir.join("assets/fonts/DroidSans.ttf"))
        .unwrap();
    let params: toml::Table = toml::from_str(
        "text = \"Project fonts\"\nsize = 64\ncolor = \"#264653\"\nfont = \"Droid Sans\"",
    )
    .unwrap();
    let mut ctx = EvalCtx {
        node_id: "expected",
        root: &dir,
        gui: &mut NullHost,
    };
    let expected = Text::with_fonts(db)
        .eval(&Inputs::default(), &params, &mut ctx)
        .unwrap();
    let Some(Value::Layer(expected)) = expected.values.get("out") else {
        panic!("text produces a layer");
    };
    assert_eq!(title.image.dimensions(), expected.image.dimensions());
    assert_eq!(title.image.as_raw(), expected.image.as_raw());
}
