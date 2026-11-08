//! Spotlight (`s`): tints what the focused node draws in the preview, following focus, so
//! you can see which part of the output a node is responsible for. Nothing is written.

use conduit_core::ValueType;

use super::App;

impl App {
    /// `s`: turns the spotlight on or off.
    pub(super) fn toggle_spotlight(&mut self) {
        self.spotlight = !self.spotlight;
        self.message = Some(if self.spotlight {
            "spotlight on: what the focus draws is tinted in the preview".into()
        } else {
            "spotlight off".into()
        });
    }

    /// The nodes to tint now: the node the focus is about (a conduit or input end means
    /// its source), or, if that draws nothing (a number, color or font), the nodes it
    /// feeds. Empty when the spotlight is off.
    pub fn spotlight_targets(&self) -> Vec<String> {
        if !self.spotlight {
            return Vec::new();
        }
        let target = if self.flow_view {
            self.flow_probe_target()
        } else {
            self.probe_target()
        };
        let Some(id) = target.map(|r| r.node) else {
            return Vec::new();
        };
        let draws = self.project.get(&id).is_some_and(|n| {
            n.outputs()
                .iter()
                .any(|(_, ty)| matches!(ty, ValueType::Image | ValueType::Layer))
        });
        if draws {
            return vec![id];
        }
        let mut users: Vec<String> = self
            .project
            .consumers(&id)
            .iter()
            .map(|c| c.node.clone())
            .collect();
        users.sort();
        users.dedup();
        users
    }

    /// The spotlight's targets if they changed since the last call, to send to the
    /// renderer.
    pub fn spotlight_changed(&mut self) -> Option<Vec<String>> {
        let targets = self.spotlight_targets();
        if targets == self.spotlight_sent {
            return None;
        }
        self.spotlight_sent = targets.clone();
        Some(targets)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use conduit_core::inspect;
    use ratatui::crossterm::event::{KeyCode, KeyEvent};

    use super::*;

    fn app(name: &str) -> App {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples")
            .join(name);
        let registry = Arc::new(conduit_nodes::registry());
        let project = inspect(&dir, &registry).unwrap();
        App::new(dir, project, registry)
    }

    fn press(app: &mut App, keys: &str) {
        for c in keys.chars() {
            app.handle_key(KeyEvent::from(KeyCode::Char(c)));
        }
    }

    #[test]
    fn follows_the_focus_and_turns_off() {
        let mut app = app("basic");
        assert_eq!(app.spotlight_changed(), None, "off by default");
        // On the preview, which draws nothing itself: what feeds it.
        press(&mut app, "s");
        assert_eq!(app.spotlight_changed(), Some(vec!["comp".into()]));
        assert_eq!(app.spotlight_changed(), None, "sent only when it changes");
        press(&mut app, "hhh");
        let moved = app.spotlight_changed().expect("follows the focus");
        assert_ne!(moved, ["comp"]);
        press(&mut app, "s");
        assert_eq!(app.spotlight_changed(), Some(vec![]));
    }

    #[test]
    fn a_color_lights_up_what_it_colors() {
        let mut app = app("effects");
        app.focus = Some("brand".into());
        app.sync_flow();
        press(&mut app, "s");
        let targets = app.spotlight_targets();
        assert!(!targets.is_empty());
        assert!(!targets.contains(&"brand".to_string()));
    }
}
