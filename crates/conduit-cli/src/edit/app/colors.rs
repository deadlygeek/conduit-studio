//! The color editor overlay (HSL / RGB / alpha channels, nudged with the usual keys, or a
//! typed hex) and palette color actions.

use conduit_core::edit::{self as ops, Change};
use conduit_core::{Channel, ColorModel, ParamKind, Rgba, parse_color, to_hex};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::{Action, App, Confirm, Mode, Prompt, Row, nudge_step};
use crate::run::Shown;

/// What the color editor writes to.
#[derive(Debug, Clone, PartialEq)]
pub enum ColorTarget {
    /// A color param (writing it replaces any link).
    Param { node: String, name: String },
    /// A palette color.
    Entry {
        node: String,
        param: String,
        key: String,
    },
}

impl ColorTarget {
    pub fn label(&self) -> String {
        match self {
            ColorTarget::Param { node, name } => format!("{node}.{name}"),
            ColorTarget::Entry { node, key, .. } => format!("{node}.{key}"),
        }
    }
}

/// State of the color editor overlay.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorEdit {
    pub target: ColorTarget,
    pub original: Rgba<u8>,
    pub model: ColorModel,
    /// 0..=3: H, S, L, A (or R, G, B, A).
    pub channel: usize,
    pub rgb: bool,
    /// Text being typed after `#`.
    pub hex: Option<String>,
    /// Whether this session has written anything (and so owns the last undo entry).
    pub changed: bool,
}

impl ColorEdit {
    fn new(target: ColorTarget, color: Rgba<u8>) -> Self {
        Self {
            target,
            original: color,
            model: ColorModel::new(color),
            channel: 0,
            rgb: false,
            hex: None,
            changed: false,
        }
    }

    pub fn color(&self) -> Rgba<u8> {
        self.model.color
    }

    /// The four channels on show: H S L A, or R G B A.
    pub fn channels(&self) -> [Channel; 4] {
        if self.rgb {
            Channel::RGBA
        } else {
            Channel::HSLA
        }
    }

    fn nudge(&mut self, delta: i64) {
        let ch = self.channels()[self.channel];
        self.model.nudge(ch, delta);
    }
}

impl App {
    /// `⏎` on a color param or palette color: open the color editor.
    pub(super) fn open_color_editor(&mut self) -> bool {
        let Some(node) = self.focused() else {
            return false;
        };
        let (target, start) = match self.current_row() {
            Some(Row::Param { name, link }) => {
                let is_color = node
                    .spec
                    .as_ref()
                    .and_then(|s| s.param(name))
                    .is_some_and(|p| p.kind == ParamKind::Color);
                if !is_color {
                    return false;
                }
                let linked = link.as_ref().and_then(|l| match self.linked_value(l) {
                    Some(Shown::Color(c)) => Some(c),
                    _ => None,
                });
                let typed = self.current_value(name).and_then(|v| color_of(&v));
                let start = linked.or(typed).unwrap_or(Rgba([255, 255, 255, 255]));
                let target = ColorTarget::Param {
                    node: node.id.clone(),
                    name: name.clone(),
                };
                (target, start)
            }
            Some(Row::Entry { param, key }) => {
                let value = node
                    .params
                    .get(param)
                    .and_then(|t| t.get(key))
                    .and_then(color_of);
                let target = ColorTarget::Entry {
                    node: node.id.clone(),
                    param: param.clone(),
                    key: key.clone(),
                };
                (target, value.unwrap_or(Rgba([255, 255, 255, 255])))
            }
            _ => return false,
        };
        // A new editing session: its writes undo together, separately from earlier ones.
        self.nudging = None;
        self.mode = Mode::Color(Box::new(ColorEdit::new(target, start)));
        true
    }

    /// Keys while the color editor is open (mode has been reset to `Normal`).
    pub(super) fn color_key(&mut self, key: KeyEvent, mut edit: Box<ColorEdit>) {
        if let Some(mut hex) = edit.hex.take() {
            match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => match parse_color(&hex) {
                    Ok(c) => {
                        edit.model.set(c);
                        self.write_color(&mut edit);
                    }
                    Err(e) => {
                        self.message = Some(e);
                        edit.hex = Some(hex);
                    }
                },
                KeyCode::Backspace => {
                    hex.pop();
                    edit.hex = Some(hex);
                }
                KeyCode::Char(c) if !c.is_control() => {
                    hex.push(c);
                    edit.hex = Some(hex);
                }
                _ => edit.hex = Some(hex),
            }
            self.mode = Mode::Color(edit);
            return;
        }
        match key.code {
            KeyCode::Enter => {
                self.message = Some(format!(
                    "{} = {}",
                    edit.target.label(),
                    to_hex(edit.color())
                ));
                return;
            }
            KeyCode::Esc => {
                self.revert_color(&edit);
                return;
            }
            KeyCode::Char('j') | KeyCode::Down => edit.channel = (edit.channel + 1).min(3),
            KeyCode::Char('k') | KeyCode::Up => edit.channel = edit.channel.saturating_sub(1),
            KeyCode::Tab => edit.rgb = !edit.rgb,
            KeyCode::Char('#') => edit.hex = Some("#".into()),
            KeyCode::Char(c) => {
                if let Some(delta) = nudge_step(c) {
                    edit.nudge(delta);
                    self.write_color(&mut edit);
                }
            }
            _ => {}
        }
        self.mode = Mode::Color(edit);
    }

    /// Writes the editor's color; all writes of one session undo as one step.
    fn write_color(&mut self, edit: &mut ColorEdit) {
        let hex = toml::Value::String(to_hex(edit.color()));
        let result = match &edit.target {
            ColorTarget::Param { node, name } => ops::set_param(&self.dir, node, name, &hex),
            ColorTarget::Entry { node, param, key } => {
                ops::set_entry(&self.dir, node, param, key, &hex)
            }
        };
        match result {
            Ok(change) => {
                let label = format!("set {} = {hex}", edit.target.label());
                self.record_coalesced(format!("color {}", edit.target.label()), label, change);
                edit.changed = true;
            }
            Err(e) => self.message = Some(format!("error: {e}")),
        }
    }

    /// `esc`: put back what was there before the editor opened (including a link).
    fn revert_color(&mut self, edit: &ColorEdit) {
        let key = format!("color {}", edit.target.label());
        if !edit.changed || self.nudging.as_ref() != Some(&key) {
            return;
        }
        if let Some((_, change)) = self.undo.pop() {
            self.nudging = None;
            match change.undo() {
                Ok(()) => self.message = Some(format!("reverted {}", edit.target.label())),
                Err(e) => self.message = Some(format!("revert failed: {e}")),
            }
            self.reload_from_disk();
        }
    }

    /// Records an edit for undo, merging it into the previous one when `key` matches the
    /// previous coalesced edit (repeated nudges, a color editing session).
    pub(super) fn record_coalesced(&mut self, key: String, label: String, change: Change) {
        // Keep the first change: it holds the contents from before the first edit.
        match self.undo.last_mut() {
            Some(last) if self.nudging.as_ref() == Some(&key) => last.0 = label.clone(),
            _ => self.undo.push((label.clone(), change)),
        }
        self.nudging = Some(key);
        self.reload_from_disk();
        self.message = Some(label);
    }

    /// `C` on a color param or palette color: open the GUI picker for it. The node file is
    /// snapshotted first, so `u` undoes whatever the picker does.
    pub(super) fn open_picker(&mut self) -> Option<Action> {
        let node = self.focused()?;
        let (name, table) = match self.current_row() {
            Some(Row::Param { name, .. })
                if node
                    .spec
                    .as_ref()
                    .and_then(|s| s.param(name))
                    .is_some_and(|p| p.kind == ParamKind::Color) =>
            {
                (name.clone(), None)
            }
            Some(Row::Entry { param, key }) => (key.clone(), Some(param.clone())),
            _ => {
                self.message = Some("move the cursor to a color to open the picker".into());
                return None;
            }
        };
        let label = format!("{}.{name}", node.id);
        let mut args = vec![
            self.dir.to_string_lossy().into_owned(),
            node.id.clone(),
            name,
        ];
        if let Some(table) = table {
            args.extend(["--table".to_string(), table]);
        }
        match ops::snapshot(std::slice::from_ref(&node.path)) {
            Ok(change) => {
                self.undo.push((format!("color picker on {label}"), change));
                self.nudging = None;
            }
            Err(e) => self.message = Some(format!("error: {e}")),
        }
        self.message = Some(format!("opened the color picker for {label}"));
        Some(Action::OpenPicker(args))
    }

    // --- palette colors ----------------------------------------------------------------

    /// The focused node's table-of-colors param (a palette's `colors`), if any.
    pub(super) fn entries_param(&self) -> Option<String> {
        let spec = self.focused()?.spec.as_ref()?;
        let p = spec.params.iter().find(|p| p.kind == ParamKind::Colors)?;
        Some(p.name.to_string())
    }

    /// `o`: add a color to the focused palette, starting as a copy of the one under the
    /// cursor.
    pub(super) fn start_add_entry(&mut self) {
        let Some(param) = self.entries_param() else {
            self.message = Some("o adds a color to a palette".into());
            return;
        };
        let node = self.focused().expect("has a palette param");
        let taken = |k: &str| node.params.get(&param).and_then(|t| t.get(k)).is_some();
        let name = (1..)
            .map(|n| format!("c{n}"))
            .find(|k| !taken(k))
            .expect("unbounded");
        self.open_prompt(Prompt::AddEntry { param }, name);
    }

    /// `x` on a palette color: remove it (after confirmation if something uses it).
    pub(super) fn start_remove_entry(&mut self) {
        let (Some(Row::Entry { param, key }), Some(focus)) =
            (self.current_row().cloned(), self.focus.clone())
        else {
            self.message = Some("move the cursor to a palette color to remove it".into());
            return;
        };
        let users: Vec<String> = self
            .project
            .consumers(&focus)
            .iter()
            .filter(|c| c.output == key)
            .map(|c| format!("{}.{}", c.node, c.port))
            .collect();
        if users.is_empty() {
            self.remove_entry(&param, &key);
        } else {
            self.mode = Mode::Confirm {
                question: format!(
                    "Remove `{focus}.{key}`? This disconnects {}.",
                    users.join(", ")
                ),
                purpose: Confirm::RemoveEntry { param, key },
            };
        }
    }

    pub(super) fn remove_entry(&mut self, param: &str, key: &str) {
        let Some(focus) = self.focus.clone() else {
            return;
        };
        self.apply_or_report(format!("removed {focus}.{key}"), |dir, project| {
            ops::remove_entry(dir, project, &focus, param, key)
        });
    }

    /// The color under the cursor (palette row), for seeding a new color.
    pub(super) fn entry_color(&self) -> Option<String> {
        let Some(Row::Entry { param, key }) = self.current_row() else {
            return None;
        };
        let value = self.focused()?.params.get(param)?.get(key)?;
        value.as_str().map(str::to_string)
    }
}

fn color_of(v: &toml::Value) -> Option<Rgba<u8>> {
    v.as_str().and_then(|s| parse_color(s).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_follow_the_mode() {
        let target = ColorTarget::Param {
            node: "n".into(),
            name: "color".into(),
        };
        let mut e = ColorEdit::new(target, parse_color("#ff0000").unwrap());
        e.nudge(120);
        assert_eq!(to_hex(e.color()), "#00ff00", "hue first");
        e.rgb = true;
        e.channel = 2;
        e.nudge(-255);
        assert_eq!(to_hex(e.color()), "#00ff00", "blue already 0");
        e.channel = 0;
        e.nudge(50);
        assert_eq!(to_hex(e.color()), "#32ff00");
    }
}
