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
        "type = \"position\"\n[inputs]\nimage = \"ghost\"",
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
        stderr.contains("b.toml: input `image` refers to missing node `ghost`"),
        "{stderr}"
    );
    assert!(stderr.contains("2 problem(s) found"), "{stderr}");
}

#[test]
fn nodes_lists_builtins() {
    let out = conduit(&["nodes"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    for kind in ["rect", "position", "composite"] {
        assert!(
            stdout.lines().any(|l| l == kind),
            "missing {kind}: {stdout}"
        );
    }
}
