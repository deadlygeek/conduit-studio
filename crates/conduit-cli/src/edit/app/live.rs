//! Pickers that preview in place (fonts, files): moving through the list writes the
//! highlighted value to the node file, so the preview shows it; `⏎` keeps it and `esc` puts
//! back what was there. One picker session undoes as one step.

use conduit_core::edit as ops;

use super::{App, Mode, Pick};

impl App {
    /// The focused node's text param as written (or its default).
    pub(super) fn text_value(&self, param: &str) -> Option<String> {
        match self.current_value(param) {
            Some(toml::Value::String(s)) => Some(s),
            _ => None,
        }
    }

    fn session_key(param: &str, node: &str) -> String {
        format!("pick {node}.{param}")
    }

    /// Writes the highlighted choice so the preview shows it. Called after every move.
    pub(super) fn preview_pick(&mut self, param: &str) {
        let Mode::Pick { selected, .. } = &self.mode else {
            return;
        };
        if let Some((value, _)) = self.pick_matches().into_iter().nth(*selected) {
            self.write_live(param, &value);
        }
    }

    /// `⏎`: keep the chosen value (already written while previewing).
    pub(super) fn keep_pick(&mut self, param: &str, value: &str) {
        self.write_live(param, value);
        let node = self.focus.clone().unwrap_or_default();
        self.message = Some(format!("set {node}.{param} = \"{value}\""));
    }

    /// Writes `param = value` as part of this picker session (one undo step).
    fn write_live(&mut self, param: &str, value: &str) {
        let Some(node) = self.focus.clone() else {
            return;
        };
        if self.text_value(param).as_deref() == Some(value) {
            return;
        }
        let result = ops::set_param(&self.dir, &node, param, &toml::Value::String(value.into()));
        let label = format!("set {node}.{param} = \"{value}\"");
        self.write_session(Self::session_key(param, &node), label, result);
    }

    /// `esc`: put back the value from before the picker opened.
    pub(super) fn revert_pick(&mut self, param: &str) {
        let node = self.focus.clone().unwrap_or_default();
        self.revert_session(&Self::session_key(param, &node), &format!("{node}.{param}"));
    }

    /// Opens a picker session on `purpose`, with `current` highlighted.
    pub(super) fn open_live_pick(&mut self, purpose: Pick, current: Option<&str>) {
        // A new session: its writes undo together, separately from earlier edits.
        self.nudging = None;
        let selected = self
            .pick_items(&purpose)
            .iter()
            .position(|(key, _)| Some(key.as_str()) == current)
            .unwrap_or(0);
        self.mode = Mode::Pick {
            purpose,
            query: String::new(),
            selected,
        };
    }
}
