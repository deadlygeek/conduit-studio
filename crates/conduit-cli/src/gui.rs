//! Launching and feeding GUI node processes (`conduit-preview`).

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;

use conduit_core::{Graph, GuiHost, Registry, RgbaImage};

/// Overrides the GUI program; used by tests and handy for trying alternative previewers.
pub const PROGRAM_ENV: &str = "CONDUIT_PREVIEW_BIN";

/// Overrides the color picker program.
pub const PICKER_ENV: &str = "CONDUIT_PICKER_BIN";

/// Finds `conduit-preview`: `$CONDUIT_PREVIEW_BIN`, else next to this executable, else `$PATH`.
pub fn preview_program() -> PathBuf {
    find_program(PROGRAM_ENV, "conduit-preview")
}

/// Finds `conduit-picker`, the same way as [`preview_program`] (`$CONDUIT_PICKER_BIN`).
pub fn picker_program() -> PathBuf {
    find_program(PICKER_ENV, "conduit-picker")
}

fn find_program(env: &str, name: &str) -> PathBuf {
    if let Some(p) = std::env::var_os(env) {
        return p.into();
    }
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(name)))
        .filter(|p| p.is_file());
    sibling.unwrap_or_else(|| PathBuf::from(name))
}

struct GuiChild {
    child: Child,
    stdin: ChildStdin,
}

/// One child process per GUI node, spawned on first use and respawned after the user
/// closes its window. Implements [`GuiHost`] so GUI nodes can send it frames.
pub struct GuiProcesses {
    program: PathBuf,
    save_dir: PathBuf,
    /// Window title for each GUI node in the current graph.
    titles: HashMap<String, String>,
    children: HashMap<String, GuiChild>,
    /// Discard the windows' stderr (needed when a TUI owns the terminal).
    quiet: bool,
    /// Problems starting windows, for the caller to report.
    warnings: Vec<String>,
}

impl GuiProcesses {
    pub fn new(program: PathBuf, save_dir: PathBuf) -> Self {
        Self {
            program,
            save_dir,
            titles: HashMap::new(),
            children: HashMap::new(),
            quiet: false,
            warnings: Vec::new(),
        }
    }

    pub fn quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }

    /// Sets the title used when the window for `id` is (re)opened.
    pub fn set_title(&mut self, id: &str, title: String) {
        self.titles.insert(id.to_string(), title);
    }

    /// Problems since the last call (e.g. the preview program couldn't be started).
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    /// Closes every window.
    pub fn close_all(&mut self) {
        self.children.clear();
    }

    /// Records the GUI nodes in a freshly loaded graph and closes windows whose node is gone.
    pub fn sync(&mut self, graph: &Graph, registry: &Registry) {
        self.titles = graph
            .nodes
            .iter()
            .filter(|n| registry.spec(&n.kind).is_some_and(|s| s.gui))
            .map(|n| {
                let title = n
                    .params
                    .get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or(&n.id);
                (n.id.clone(), title.to_string())
            })
            .collect();
        self.children.retain(|id, _| self.titles.contains_key(id));
    }

    /// Shows an error in the window for `id`, opening it if needed.
    pub fn show_error_in(&mut self, id: &str, message: &str) {
        let Some(gui) = self.ensure(id) else { return };
        if conduit_ipc::write_error(&mut gui.stdin, message).is_err() {
            self.children.remove(id);
        }
    }

    /// Shows an error in every open window (they keep their last good image).
    pub fn show_error(&mut self, message: &str) {
        let mut failed = Vec::new();
        for (id, gui) in &mut self.children {
            if conduit_ipc::write_error(&mut gui.stdin, message).is_err() {
                failed.push(id.clone());
            }
        }
        for id in failed {
            self.children.remove(&id);
        }
    }

    /// Returns a live process for `node_id`, spawning one if needed.
    fn ensure(&mut self, node_id: &str) -> Option<&mut GuiChild> {
        let alive = self
            .children
            .get_mut(node_id)
            .is_some_and(|g| matches!(g.child.try_wait(), Ok(None)));
        if !alive {
            self.children.remove(node_id);
            let gui = self.spawn(node_id)?;
            self.children.insert(node_id.to_string(), gui);
        }
        self.children.get_mut(node_id)
    }

    fn spawn(&mut self, node_id: &str) -> Option<GuiChild> {
        let title = self.titles.get(node_id).map_or(node_id, String::as_str);
        let args: [OsString; 6] = [
            "--node-id".into(),
            node_id.into(),
            "--title".into(),
            title.into(),
            "--save-dir".into(),
            self.save_dir.clone().into(),
        ];
        let mut child = match Command::new(&self.program)
            .args(&args)
            .stdin(Stdio::piped())
            .stderr(if self.quiet {
                Stdio::null()
            } else {
                Stdio::inherit()
            })
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                self.warnings.push(format!(
                    "cannot start {}: {e}\n  (build it with `cargo build --workspace`, or set {PROGRAM_ENV})",
                    self.program.display()
                ));
                return None;
            }
        };
        let stdin = child.stdin.take().expect("stdin was piped");
        Some(GuiChild { child, stdin })
    }
}

impl GuiHost for GuiProcesses {
    fn show(&mut self, node_id: &str, image: Arc<RgbaImage>) {
        let Some(gui) = self.ensure(node_id) else {
            return;
        };
        let sent = conduit_ipc::write_frame(
            &mut gui.stdin,
            image.width(),
            image.height(),
            image.as_raw(),
        );
        if sent.is_err() {
            // Window was closed while we were writing; it will be reopened on the next change.
            self.children.remove(node_id);
        }
    }
}

impl Drop for GuiProcesses {
    fn drop(&mut self) {
        // Closing stdin makes each window exit.
        for (_, gui) in self.children.drain() {
            let GuiChild { mut child, stdin } = gui;
            drop(stdin);
            let _ = child.wait();
        }
    }
}
