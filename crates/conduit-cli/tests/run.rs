//! End-to-end test of `conduit run` watch mode, with a shell script standing in for
//! `conduit-preview` that records the messages it receives.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use conduit_ipc::{Msg, read_msg};

const TIMEOUT: Duration = Duration::from_secs(15);

struct Runner(Child);

impl Drop for Runner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Every complete message in the recording so far.
fn messages(path: &Path) -> Vec<Msg> {
    let bytes = std::fs::read(path).unwrap_or_default();
    let mut r = bytes.as_slice();
    let mut msgs = Vec::new();
    while let Ok(Some(msg)) = read_msg(&mut r) {
        msgs.push(msg);
    }
    msgs
}

/// Waits for a message at index `from` or later matching `pred`; returns its index.
/// (One file save can trigger more than one re-render, so counting messages is unreliable.)
fn wait_for(path: &Path, from: usize, what: &str, pred: impl Fn(&Msg) -> bool) -> (usize, Msg) {
    let start = Instant::now();
    loop {
        let msgs = messages(path);
        if let Some(i) = (from..msgs.len()).find(|&i| pred(&msgs[i])) {
            return (i, msgs[i].clone());
        }
        assert!(
            start.elapsed() < TIMEOUT,
            "timed out waiting for {what}; got {msgs:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn pixel(msg: &Msg, x: u32, y: u32) -> [u8; 4] {
    let Msg::Frame { width, rgba, .. } = msg else {
        panic!("expected frame, got {msg:?}")
    };
    let i = ((y * width + x) * 4) as usize;
    rgba[i..i + 4].try_into().unwrap()
}

#[test]
fn run_renders_and_rerenders_on_change() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let example = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/basic");
    for entry in std::fs::read_dir(example).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "toml") {
            std::fs::copy(&path, project.join(path.file_name().unwrap())).unwrap();
        }
    }

    let frames = tmp.path().join("frames.bin");
    let args = tmp.path().join("args.txt");
    let script = tmp.path().join("fake-preview.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$ARGS_FILE\"\nexec cat > \"$FRAMES_FILE\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

    let _runner = Runner(
        Command::new(env!("CARGO_BIN_EXE_conduit"))
            .arg("run")
            .arg(&project)
            .env("CONDUIT_PREVIEW_BIN", &script)
            .env("FRAMES_FILE", &frames)
            .env("ARGS_FILE", &args)
            .spawn()
            .unwrap(),
    );

    // First render: black square at (300, 300).
    let (first, msg) = wait_for(&frames, 0, "first frame", |_| true);
    assert_eq!(first, 0);
    assert!(matches!(
        msg,
        Msg::Frame {
            width: 800,
            height: 600,
            ..
        }
    ));
    assert_eq!(pixel(&msg, 300, 300), [0, 0, 0, 255]);
    assert_eq!(pixel(&msg, 100, 300), [255, 255, 255, 255]);

    // Nothing changed, so nothing more is sent (our own reads of the files must not
    // trigger re-renders).
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(messages(&frames).len(), 1, "re-rendered without changes");

    let args = std::fs::read_to_string(&args).unwrap();
    let args: Vec<&str> = args.lines().collect();
    assert_eq!(args[..4], ["--node-id", "preview", "--title", "basic"]);
    assert_eq!(args[4], "--save-dir");
    assert!(args[5].ends_with("project/out"), "{}", args[5]);

    // Move the square: the same window gets a new frame.
    let placed = project.join("placed.toml");
    let text = std::fs::read_to_string(&placed)
        .unwrap()
        .replace("x = 300", "x = 100");
    std::fs::write(&placed, text).unwrap();
    let (moved, msg) = wait_for(&frames, 1, "frame with moved square", |m| {
        matches!(m, Msg::Frame { .. }) && pixel(m, 100, 300) == [0, 0, 0, 255]
    });
    assert_eq!(pixel(&msg, 300, 300), [255, 255, 255, 255]);

    // Break a file: the window gets an error naming the problem.
    std::fs::write(
        project.join("comp.toml"),
        "type = \"composite\"\n[inputs]\nbase = \"ghost\"\n",
    )
    .unwrap();
    wait_for(
        &frames,
        moved + 1,
        "error naming `ghost`",
        |m| matches!(m, Msg::Error(e) if e.contains("comp.toml: input `base` refers to missing node `ghost`")),
    );
}
