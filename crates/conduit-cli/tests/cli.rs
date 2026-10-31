use std::path::PathBuf;
use std::process::{Command, Output};

fn conduit(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_conduit"))
        .args(args)
        .output()
        .expect("run conduit")
}

fn example_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/basic")
}

#[test]
fn check_example_succeeds() {
    let out = conduit(&["check", example_dir().to_str().unwrap()]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("comp.out"), "{stdout}");
    assert!(stdout.contains("image 800x600"), "{stdout}");
    assert!(stdout.contains("layer 50x50 at 300,300"), "{stdout}");
}

#[test]
fn check_reports_all_problems() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("a.toml"),
        "type = \"rect\"\n[params]\ncolor = \"nope\"\nwidth = 1\nheight = 1",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("b.toml"),
        "type = \"transform\"\n[inputs]\nlayer = \"ghost\"",
    )
    .unwrap();
    let out = conduit(&["check", dir.path().to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success());
    assert!(
        stderr.contains("a.toml: params: unknown color `nope`"),
        "{stderr}"
    );
    assert!(
        stderr.contains("b.toml: input `layer` refers to missing node `ghost`"),
        "{stderr}"
    );
    assert!(stderr.contains("2 problem(s) found"), "{stderr}");
}

#[test]
fn nodes_lists_builtins() {
    let out = conduit(&["nodes"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    for kind in [
        "rect",
        "text",
        "transform",
        "realize",
        "hsl",
        "tint",
        "invert",
        "composite",
    ] {
        assert!(
            stdout.lines().any(|l| l == kind),
            "missing {kind}: {stdout}"
        );
    }
}

#[test]
fn fonts_lists_generic_families() {
    let out = conduit(&["fonts"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    assert!(stdout.contains("sans-serif"), "{stdout}");
}

#[test]
fn palette_import() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_str().unwrap();
    let url = "https://coolors.co/264653-2a9d8f-e9c46a";
    let out = conduit(&["palette", "import", dir, "brand", url]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = std::fs::read_to_string(tmp.path().join("brand.toml")).unwrap();
    assert!(text.contains(&format!("source = \"{url}\"")), "{text}");
    assert!(text.contains("c3 = \"#e9c46a\""), "{text}");

    // From a file, replacing the colors; the names in a CSS export are kept.
    let css = tmp.path().join("export.css");
    std::fs::write(&css, "--charcoal: #264653ff;\n--sandy-brown: #f4a261ff;\n").unwrap();
    let out = conduit(&["palette", "import", dir, "brand", css.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let check = conduit(&["check", dir]);
    let stdout = String::from_utf8_lossy(&check.stdout);
    assert!(stdout.contains("brand.sandy_brown"), "{stdout}");
    assert!(!stdout.contains("brand.c1"), "{stdout}");

    let out = conduit(&["palette", "import", dir, "brand", "no colors here"]);
    assert!(!out.status.success());
}
