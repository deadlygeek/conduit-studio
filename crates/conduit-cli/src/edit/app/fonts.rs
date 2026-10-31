//! The font picker: `⏎` on a font param lists the installed families. Moving through the
//! list writes the highlighted font to the node file, so the preview shows it in place;
//! `⏎` keeps it and `esc` puts back what was there. One session undoes as one step.

use std::sync::Arc;

use conduit_core::ParamKind;
use conduit_core::edit as ops;
use conduit_nodes::text::{FontFamily, GENERIC_FAMILIES};

use super::{App, Mode, Pick, Row};

/// How many recently chosen fonts are listed first.
const RECENT: usize = 5;

impl App {
    /// `⏎` on a font param: open the font picker on the current font.
    pub(super) fn open_font_picker(&mut self) -> bool {
        let Some(Row::Param { name, .. }) = self.current_row().cloned() else {
            return false;
        };
        let is_font = self
            .focused()
            .and_then(|n| n.spec.as_ref())
            .and_then(|s| s.param(&name))
            .is_some_and(|p| p.kind == ParamKind::Font);
        if !is_font {
            return false;
        }
        // Installed families load the first time the picker opens.
        if self.fonts.is_none() {
            self.fonts = Some(Arc::new(conduit_nodes::text::font_families()));
        }
        // A new session: its writes undo together, separately from earlier edits.
        self.nudging = None;
        let purpose = Pick::Font {
            param: name.clone(),
        };
        let current = self.font_value(&name);
        let selected = self
            .pick_items(&purpose)
            .iter()
            .position(|(key, _)| Some(key) == current.as_ref())
            .unwrap_or(0);
        self.mode = Mode::Pick {
            purpose,
            query: String::new(),
            selected,
        };
        true
    }

    /// The picker's choices: recently chosen fonts, the generic names, then every
    /// installed family. Labels are "name  notes" (the notes are dimmed and searchable).
    pub(super) fn font_items(&self, param: &str) -> Vec<(String, String)> {
        let families = self.fonts.clone().unwrap_or_default();
        let current = self.font_value(param);
        let italic = self
            .focused()
            .is_some_and(|n| n.params.get("style").and_then(|v| v.as_str()) == Some("italic"));
        let notes = |name: &str, base: Vec<&str>| {
            let mut notes = base;
            if current.as_deref() == Some(name) {
                notes.push("current");
            }
            notes.join(" · ")
        };
        let family_notes = |f: &FontFamily| {
            let mut base = Vec::new();
            if f.monospace {
                base.push("mono");
            }
            if italic && !f.italic {
                base.push("no italic");
            }
            notes(&f.name, base)
        };
        let lookup = |name: &str| families.iter().find(|f| f.name == name);

        let mut items = Vec::new();
        let mut listed = Vec::new();
        for name in self.recent_fonts.iter().rev() {
            let base = match lookup(name) {
                Some(f) => family_notes(f),
                None if GENERIC_FAMILIES.contains(&name.as_str()) => notes(name, vec!["generic"]),
                None => continue,
            };
            let base = if base.is_empty() {
                "recent".into()
            } else {
                format!("recent · {base}")
            };
            items.push((name.clone(), format!("{name}  {base}")));
            listed.push(name.clone());
        }
        for name in GENERIC_FAMILIES
            .iter()
            .filter(|n| !listed.iter().any(|l| l == *n))
        {
            items.push((
                name.to_string(),
                format!("{name}  {}", notes(name, vec!["generic"])),
            ));
        }
        for f in families.iter().filter(|f| !listed.contains(&f.name)) {
            let notes = family_notes(f);
            let label = if notes.is_empty() {
                f.name.clone()
            } else {
                format!("{}  {notes}", f.name)
            };
            items.push((f.name.clone(), label));
        }
        items
    }

    /// The focused node's font param as written (or its default).
    fn font_value(&self, param: &str) -> Option<String> {
        match self.current_value(param) {
            Some(toml::Value::String(s)) => Some(s),
            _ => None,
        }
    }

    fn session_key(param: &str, node: &str) -> String {
        format!("font {node}.{param}")
    }

    /// Writes the highlighted font so the preview shows it. Called after every move.
    pub(super) fn preview_font(&mut self, param: &str) {
        let Mode::Pick { selected, .. } = &self.mode else {
            return;
        };
        if let Some((name, _)) = self.pick_matches().into_iter().nth(*selected) {
            self.write_font(param, &name);
        }
    }

    /// `⏎`: keep the chosen font (already written while previewing).
    pub(super) fn choose_font(&mut self, param: &str, name: String) {
        self.write_font(param, &name);
        self.recent_fonts.retain(|f| *f != name);
        self.recent_fonts.push(name.clone());
        if self.recent_fonts.len() > RECENT {
            self.recent_fonts.remove(0);
        }
        let node = self.focus.clone().unwrap_or_default();
        self.message = Some(format!("set {node}.{param} = \"{name}\""));
    }

    /// Writes `param = name` as part of this picker session (one undo step).
    fn write_font(&mut self, param: &str, name: &str) {
        let Some(node) = self.focus.clone() else {
            return;
        };
        if self.font_value(param).as_deref() == Some(name) {
            return;
        }
        match ops::set_param(&self.dir, &node, param, &toml::Value::String(name.into())) {
            Ok(change) => {
                let label = format!("set {node}.{param} = \"{name}\"");
                self.record_coalesced(Self::session_key(param, &node), label, change);
            }
            Err(e) => self.message = Some(format!("error: {e}")),
        }
    }

    /// `esc`: put back the font from before the picker opened.
    pub(super) fn revert_font(&mut self, param: &str) {
        let node = self.focus.clone().unwrap_or_default();
        if self.nudging.as_deref() != Some(Self::session_key(param, &node).as_str()) {
            return;
        }
        if let Some((_, change)) = self.undo.pop() {
            self.nudging = None;
            match change.undo() {
                Ok(()) => self.message = Some(format!("reverted {node}.{param}")),
                Err(e) => self.message = Some(format!("revert failed: {e}")),
            }
            self.reload_from_disk();
        }
    }
}
