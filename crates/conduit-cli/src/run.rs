//! The renderer behind `conduit run` and `conduit edit`: watches a project folder, re-renders
//! when node files or the files they read (e.g. images) change, feeds GUI windows, and can
//! "probe" any node's output into a window of its own. Runs on a background thread; talk to
//! it through a [`RunnerHandle`].

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use conduit_core::{
    EvalStats, Evaluator, GuiHost, Reference, Registry, Rgba, RgbaImage, Value, Values, load,
};
use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{DebounceEventResult, Debouncer, new_debouncer};

use crate::gui::{GuiProcesses, preview_program};

const DEBOUNCE: Duration = Duration::from_millis(150);
/// Window id of the probe window (it has its own `GuiProcesses`, so this can't clash with
/// a node id).
const PROBE_ID: &str = "probe";

pub enum Command {
    /// Show this output in the probe window, now and after every render.
    Probe(Reference),
    ProbeOff,
    /// Tint what these nodes draw in every window, and re-render (empty: stop). See
    /// [`Evaluator::set_highlight`].
    Highlight(Vec<String>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Node files, or files they read, changed on disk (an editor should re-read the
    /// project). Sent before the re-render.
    FilesChanged,
    Rendered {
        stats: EvalStats,
        elapsed: Duration,
        /// Every number and color output, so editors can show linked values.
        shown: HashMap<Reference, Shown>,
    },
    /// The project failed to load or render.
    Failed(String),
    /// Something went wrong that isn't the project's fault (e.g. a window couldn't open).
    Warning(String),
}

/// A small output value an editor can show inline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shown {
    Number(f64),
    Color(Rgba<u8>),
}

pub struct Options {
    pub preview_program: PathBuf,
    /// Discard GUI windows' stderr (when a TUI owns the terminal).
    pub quiet: bool,
}

enum Input {
    Files(DebounceEventResult),
    Command(Command),
    Quit,
}

pub struct RunnerHandle {
    input: Sender<Input>,
    events: Receiver<Event>,
    thread: Option<JoinHandle<()>>,
    _watcher: Debouncer<RecommendedWatcher>,
}

impl RunnerHandle {
    pub fn send(&self, command: Command) {
        let _ = self.input.send(Input::Command(command));
    }

    pub fn events(&self) -> &Receiver<Event> {
        &self.events
    }
}

impl Drop for RunnerHandle {
    fn drop(&mut self) {
        let _ = self.input.send(Input::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Starts watching and rendering `dir`. The first render happens immediately.
pub fn spawn(
    dir: &Path,
    registry: Arc<Registry>,
    options: Options,
) -> anyhow::Result<RunnerHandle> {
    let (input_tx, input_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();

    let files_tx = input_tx.clone();
    let mut watcher = new_debouncer(DEBOUNCE, move |result| {
        let _ = files_tx.send(Input::Files(result));
    })?;
    // Recursive, so images in subfolders (e.g. `assets/`) are noticed too.
    watcher
        .watcher()
        .watch(dir, RecursiveMode::Recursive)
        .map_err(|e| anyhow::anyhow!("cannot watch {}: {e}", dir.display()))?;

    let save_dir = dir.join("out");
    let runner = Runner {
        dir: dir.to_path_buf(),
        registry,
        gui: GuiProcesses::new(options.preview_program.clone(), save_dir.clone())
            .quiet(options.quiet),
        probe_gui: GuiProcesses::new(options.preview_program, save_dir).quiet(options.quiet),
        probe: None,
        evaluator: Evaluator::default(),
        values: None,
        files: Vec::new(),
        last: Snapshot::new(),
        events: event_tx,
    };
    let thread = std::thread::spawn(move || runner.run(input_rx));
    Ok(RunnerHandle {
        input: input_tx,
        events: event_rx,
        thread: Some(thread),
        _watcher: watcher,
    })
}

/// One-line summary of a render, e.g. `rendered 5 nodes (3 unchanged) in 2 ms`.
pub fn describe_render(stats: &EvalStats, elapsed: Duration) -> String {
    let unchanged = stats.nodes - stats.evaluated;
    let unchanged = if unchanged > 0 {
        format!(" ({unchanged} unchanged)")
    } else {
        String::new()
    };
    format!(
        "rendered {} nodes{unchanged} in {} ms",
        stats.nodes,
        elapsed.as_millis()
    )
}

/// `conduit run`: render, watch, and print what happens until killed.
pub fn run(dir: &Path, registry: Arc<Registry>) -> anyhow::Result<()> {
    let options = Options {
        preview_program: preview_program(),
        quiet: false,
    };
    let handle = spawn(dir, registry, options)?;
    println!(
        "watching {} — edit a .toml file to re-render, Ctrl-C to stop",
        dir.display()
    );
    for event in handle.events().iter() {
        match event {
            Event::FilesChanged => {}
            Event::Rendered { stats, elapsed, .. } => {
                println!("{}", describe_render(&stats, elapsed))
            }
            Event::Failed(message) | Event::Warning(message) => eprintln!("{message}"),
        }
    }
    Ok(())
}

struct Runner {
    dir: PathBuf,
    registry: Arc<Registry>,
    gui: GuiProcesses,
    probe_gui: GuiProcesses,
    probe: Option<Reference>,
    /// Remembers outputs between renders so unchanged nodes aren't re-run.
    evaluator: Evaluator,
    /// Outputs of the last successful render.
    values: Option<Values>,
    /// Files the nodes read, as of the last graph that loaded.
    files: Vec<PathBuf>,
    /// Node files and `files` as of the last render.
    last: Snapshot,
    events: Sender<Event>,
}

impl Runner {
    fn run(mut self, inputs: Receiver<Input>) {
        self.render();
        for input in inputs {
            match input {
                Input::Files(Ok(_)) => {
                    // The watcher also reports our own reads of the files and unrelated files
                    // (saved PNGs, swap files), so only re-render when something we read
                    // actually changed.
                    if snapshot(&self.dir, &self.files) != self.last {
                        self.emit(Event::FilesChanged);
                        self.render();
                    }
                }
                Input::Files(Err(e)) => self.emit(Event::Warning(format!("watch error: {e}"))),
                Input::Command(Command::Probe(target)) => {
                    self.probe = Some(target);
                    self.show_probe();
                }
                Input::Command(Command::ProbeOff) => {
                    self.probe = None;
                    self.probe_gui.close_all();
                }
                Input::Command(Command::Highlight(nodes)) => {
                    self.evaluator.set_highlight(nodes);
                    self.render();
                }
                Input::Quit => break,
            }
            self.flush_warnings();
        }
    }

    fn render(&mut self) {
        let start = Instant::now();
        let graph = load(&self.dir, &self.registry);
        if let Ok(graph) = &graph {
            self.files = graph.files(&self.registry);
        }
        // Taken before evaluating, so a file changed while rendering is noticed.
        self.last = snapshot(&self.dir, &self.files);
        let result = graph
            .map_err(|errors| errors.to_string())
            .and_then(|graph| {
                self.gui.sync(&graph, &self.registry);
                self.evaluator
                    .evaluate(&graph, &self.registry, &mut self.gui)
                    .map_err(|e| e.to_string())
            });
        match result {
            Ok((values, stats)) => {
                let shown = values
                    .iter()
                    .filter_map(|(r, v)| match v {
                        Value::Number(n) => Some((r.clone(), Shown::Number(*n))),
                        Value::Color(c) => Some((r.clone(), Shown::Color(*c))),
                        _ => None,
                    })
                    .collect();
                self.values = Some(values);
                self.emit(Event::Rendered {
                    stats,
                    elapsed: start.elapsed(),
                    shown,
                });
                self.show_probe();
            }
            Err(message) => {
                self.values = None;
                self.gui.show_error(&message);
                self.probe_gui.show_error(&message);
                self.emit(Event::Failed(message));
            }
        }
        self.flush_warnings();
    }

    /// Sends the probed output to the probe window: images as-is, layers as their
    /// transformed pixels (realized onto their own bounding box), colors as a solid swatch.
    fn show_probe(&mut self) {
        let Some(target) = &self.probe else { return };
        self.probe_gui
            .set_title(PROBE_ID, format!("probe: {target}"));
        let image = match self.values.as_ref().and_then(|v| v.get(target)) {
            Some(Value::Image(image)) => Ok(image.clone()),
            Some(Value::Layer(layer)) => probe_layer(layer),
            Some(Value::Color(c)) => Ok(Arc::new(RgbaImage::from_pixel(256, 256, *c))),
            Some(other) => Err(format!(
                "`{target}` is a {}, which can't be shown as an image",
                other.ty()
            )),
            None => Err(format!(
                "`{target}` has no value (no such output, or the project has errors)"
            )),
        };
        match image {
            Ok(image) => self.probe_gui.show(PROBE_ID, image),
            Err(message) => self.probe_gui.show_error_in(PROBE_ID, &message),
        }
    }

    fn flush_warnings(&mut self) {
        let mut warnings = self.gui.take_warnings();
        warnings.extend(self.probe_gui.take_warnings());
        for w in warnings {
            self.emit(Event::Warning(w));
        }
    }

    fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }
}

type Snapshot = BTreeMap<PathBuf, (u64, Option<SystemTime>)>;

/// Size and modification time of every node file and of `files` (missing ones are left
/// out, so creating one counts as a change). Uses metadata only: reading the files would
/// itself generate watch events.
fn snapshot(dir: &Path, files: &[PathBuf]) -> Snapshot {
    let node_files = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| is_node_file(p));
    node_files
        .chain(files.iter().cloned())
        .filter_map(|p| {
            let meta = std::fs::metadata(&p).ok()?;
            Some((p, (meta.len(), meta.modified().ok())))
        })
        .collect()
}

/// Whether a file in the project folder is a node file. Other files there (editor swap
/// files, saved PNGs) only trigger a re-render when a node reads them.
fn is_node_file(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "toml")
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use conduit_ipc::{Msg, read_msg};

    use super::*;

    const TIMEOUT: Duration = Duration::from_secs(15);

    /// A project copied from `examples/basic`, and a fake preview program that records
    /// each window's messages to `<window id>.bin`.
    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let project = tmp.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let example = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/basic");
        for entry in std::fs::read_dir(example).unwrap() {
            let path = entry.unwrap().path();
            if is_node_file(&path) {
                std::fs::copy(&path, project.join(path.file_name().unwrap())).unwrap();
            }
        }
        let script = tmp.path().join("fake-preview.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\nexec cat > \"$(dirname \"$0\")/$2.bin\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        (tmp, project, script)
    }

    fn expect_event(handle: &RunnerHandle, what: &str, pred: impl Fn(&Event) -> bool) {
        let start = Instant::now();
        while start.elapsed() < TIMEOUT {
            if let Ok(event) = handle.events().recv_timeout(Duration::from_millis(100))
                && pred(&event)
            {
                return;
            }
        }
        panic!("timed out waiting for {what}");
    }

    /// Waits until the window's latest message satisfies `pred`.
    fn expect_window(file: &Path, what: &str, pred: impl Fn(&Msg) -> bool) {
        let start = Instant::now();
        loop {
            let bytes = std::fs::read(file).unwrap_or_default();
            let mut r = bytes.as_slice();
            let mut last = None;
            while let Ok(Some(msg)) = read_msg(&mut r) {
                last = Some(msg);
            }
            if last.as_ref().is_some_and(&pred) {
                return;
            }
            assert!(
                start.elapsed() < TIMEOUT,
                "timed out waiting for {what}; last: {last:?}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn frame(width: u32, height: u32) -> impl Fn(&Msg) -> bool {
        move |m| matches!(m, Msg::Frame { width: w, height: h, .. } if *w == width && *h == height)
    }

    #[test]
    fn probe_follows_renders() {
        let (tmp, project, script) = setup();
        let probe_file = tmp.path().join("probe.bin");
        let options = Options {
            preview_program: script,
            quiet: true,
        };
        let handle = spawn(&project, Arc::new(conduit_nodes::registry()), options).unwrap();
        expect_event(
            &handle,
            "first render",
            |e| matches!(e, Event::Rendered { stats, .. } if stats.nodes == 5),
        );
        expect_window(
            &tmp.path().join("preview.bin"),
            "preview frame",
            frame(800, 600),
        );

        // An image output.
        handle.send(Command::Probe(Reference::parse("black_sq").unwrap()));
        expect_window(&probe_file, "probe of black_sq", frame(50, 50));

        // Switching targets reuses the window; a layer output is shown as its image.
        handle.send(Command::Probe(Reference::parse("white_bg").unwrap()));
        expect_window(&probe_file, "probe of white_bg", frame(800, 600));
        handle.send(Command::Probe(Reference::parse("placed").unwrap()));
        expect_window(&probe_file, "probe of placed", frame(50, 50));

        // The probe follows re-renders.
        let path = project.join("black_sq.toml");
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("width = 50", "width = 70");
        std::fs::write(&path, text).unwrap();
        expect_event(&handle, "files changed", |e| *e == Event::FilesChanged);
        expect_window(&probe_file, "re-rendered probe", frame(70, 50));

        // Probing something without a value shows an error in the probe window.
        handle.send(Command::Probe(Reference::parse("nope").unwrap()));
        expect_window(
            &probe_file,
            "probe error",
            |m| matches!(m, Msg::Error(e) if e.contains("`nope.out` has no value")),
        );

        handle.send(Command::ProbeOff);
        drop(handle); // stops the runner thread and closes windows
    }

    /// Highlighting re-renders the node and what it feeds, and stopping does again.
    #[test]
    fn highlight_rerenders() {
        let (_tmp, project, script) = setup();
        let options = Options {
            preview_program: script,
            quiet: true,
        };
        let handle = spawn(&project, Arc::new(conduit_nodes::registry()), options).unwrap();
        expect_event(&handle, "first render", |e| {
            matches!(e, Event::Rendered { .. })
        });
        // Only the GUI node re-runs on a plain re-render; a highlight re-runs more.
        let rerun = |e: &Event| matches!(e, Event::Rendered { stats, .. } if stats.evaluated > 1);
        handle.send(Command::Highlight(vec!["black_sq".into()]));
        expect_event(&handle, "highlighted render", rerun);
        handle.send(Command::Highlight(vec![]));
        expect_event(&handle, "unhighlighted render", rerun);
    }

    /// Changing an image a node reads re-renders, like changing a node file.
    #[test]
    fn rerenders_when_an_image_changes() {
        let (_tmp, project, script) = setup();
        std::fs::create_dir(project.join("assets")).unwrap();
        let png = project.join("assets/sq.png");
        RgbaImage::new(5, 5).save(&png).unwrap();
        std::fs::write(
            project.join("black_sq.toml"),
            "type = \"image\"\n[params]\npath = \"assets/sq.png\"",
        )
        .unwrap();
        let options = Options {
            preview_program: script,
            quiet: true,
        };
        let handle = spawn(&project, Arc::new(conduit_nodes::registry()), options).unwrap();
        expect_event(
            &handle,
            "first render",
            |e| matches!(e, Event::Rendered { stats, .. } if stats.nodes == 5),
        );

        RgbaImage::new(9, 5).save(&png).unwrap();
        expect_event(&handle, "files changed", |e| *e == Event::FilesChanged);
        // Only the image node and what depends on it re-run.
        expect_event(
            &handle,
            "re-render",
            |e| matches!(e, Event::Rendered { stats, .. } if stats.evaluated == 4),
        );
    }

    #[test]
    fn missing_preview_program_is_a_warning() {
        let (_tmp, project, _script) = setup();
        let options = Options {
            preview_program: PathBuf::from("/nonexistent/conduit-preview"),
            quiet: true,
        };
        let handle = spawn(&project, Arc::new(conduit_nodes::registry()), options).unwrap();
        expect_event(
            &handle,
            "warning",
            |e| matches!(e, Event::Warning(w) if w.contains("cannot start")),
        );
    }
}

/// A layer's pixels as they'll be drawn, without the canvas.
fn probe_layer(layer: &conduit_core::Layer) -> Result<Arc<RgbaImage>, String> {
    let limit = i64::from(conduit_nodes::MAX_DIMENSION);
    match conduit_core::raster::bounds(layer) {
        Some((x0, y0, x1, y1))
            if x1.saturating_sub(x0) > limit || y1.saturating_sub(y0) > limit =>
        {
            Err("the transformed layer is too large to show".into())
        }
        Some(_) => Ok(conduit_core::raster::realize(layer).image),
        None => Err("the layer draws nothing (empty, or scaled to zero)".into()),
    }
}
