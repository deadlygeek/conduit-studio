//! The font picker: `⏎` on a font param lists the installed families, previewed in place
//! (see `live.rs`).

use std::sync::Arc;

use conduit_core::ParamKind;
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
        let current = self.text_value(&name);
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
        let current = self.text_value(param);
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

    /// `⏎`: keep the chosen font, and list it first next time.
    pub(super) fn choose_font(&mut self, param: &str, name: String) {
        self.keep_pick(param, &name);
        self.recent_fonts.retain(|f| *f != name);
        self.recent_fonts.push(name);
        if self.recent_fonts.len() > RECENT {
            self.recent_fonts.remove(0);
        }
    }
}
