//! `conduit edit`: a keyboard-driven terminal UI for exploring and editing a project, with
//! the renderer running alongside it.

mod app;
mod canvas;
mod finder;
mod flow;
mod view;

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use conduit_core::{Registry, inspect};
use ratatui::crossterm::event::{self, Event as TermEvent, KeyEventKind};

use crate::gui::{picker_program, preview_program};
use crate::run::{self, Command, Event, Options};
use app::{Action, App};

/// How often to check for runner events while waiting for keys.
const TICK: Duration = Duration::from_millis(100);

pub fn edit(dir: &Path, registry: Arc<Registry>) -> anyhow::Result<()> {
    let project = inspect(dir, &registry).map_err(|e| anyhow::anyhow!("{e}"))?;
    let runner = run::spawn(
        dir,
        registry.clone(),
        Options {
            preview_program: preview_program(),
            quiet: true,
        },
    )?;
    let mut app = App::new(dir.to_path_buf(), project, registry.clone());
    // Color pickers opened with `C`; closed when the editor quits.
    let mut pickers: Vec<std::process::Child> = Vec::new();

    let mut terminal = ratatui::init();
    let result = (|| -> anyhow::Result<()> {
        loop {
            terminal.draw(|frame| view::draw(frame, &mut app))?;

            while let Ok(event) = runner.events().try_recv() {
                if event == Event::FilesChanged {
                    app.reload_from_disk();
                }
                app.on_runner_event(event);
            }

            pickers.retain_mut(|p| matches!(p.try_wait(), Ok(None)));
            if !event::poll(TICK)? {
                continue;
            }
            let TermEvent::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match app.handle_key(key) {
                Some(Action::Quit) => return Ok(()),
                Some(Action::Probe(target)) => runner.send(Command::Probe(target)),
                Some(Action::ProbeOff) => runner.send(Command::ProbeOff),
                Some(Action::OpenEditor(path)) => {
                    ratatui::restore();
                    let result = open_editor(&path);
                    terminal = ratatui::init();
                    app.reload_from_disk();
                    if let Err(e) = result {
                        app.message = Some(e);
                    }
                }
                Some(Action::OpenPicker(args)) => {
                    let program = picker_program();
                    match std::process::Command::new(&program)
                        .args(&args)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                    {
                        Ok(child) => pickers.push(child),
                        Err(e) => {
                            app.message = Some(format!("cannot start {}: {e}", program.display()))
                        }
                    }
                }
                None => {}
            }
        }
    })();
    for mut picker in pickers {
        let _ = picker.kill();
        let _ = picker.wait();
    }
    ratatui::restore();
    result
}

/// Runs `$VISUAL` / `$EDITOR` (default `vi`) on `path` and waits for it.
fn open_editor(path: &Path) -> Result<(), String> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".into());
    let mut words = editor.split_whitespace();
    let program = words.next().unwrap_or("vi");
    let status = std::process::Command::new(program)
        .args(words)
        .arg(path)
        .status()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited with {status}"))
    }
}
