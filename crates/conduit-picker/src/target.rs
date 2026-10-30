//! What the picker edits — a color param or a palette color in a node file — and the
//! palette colors it offers as swatches. Everything goes through the node files, so the
//! runner and `conduit edit` see changes like any other edit.

use std::path::{Path, PathBuf};

use conduit_core::edit as ops;
use conduit_core::{
    NullHost, ParamKind, Reference, Registry, Rgba, Value, evaluate, inspect, load, parse_color,
    to_hex,
};

#[derive(Debug, Clone)]
pub struct Target {
    pub dir: PathBuf,
    pub node: String,
    /// The param, or the palette color's name when `table` is set.
    pub name: String,
    /// The table param holding palette colors (`colors`), for a palette color.
    pub table: Option<String>,
}

/// What the picker starts from.
#[derive(Debug)]
pub struct Loaded {
    pub color: Rgba<u8>,
    /// The node file as it was, for `esc` (revert everything, links included).
    pub original_text: String,
    /// Every palette color in the project, as `palette.name`.
    pub swatches: Vec<(String, Rgba<u8>)>,
}

impl Target {
    pub fn label(&self) -> String {
        format!("{}.{}", self.node, self.name)
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(format!("{}.toml", self.node))
    }

    /// Reads the current color: the palette color, the linked value (by rendering the
    /// project), the typed value, or the param's example; white if none of those work.
    pub fn load(&self, registry: &Registry) -> Result<Loaded, String> {
        let path = self.path();
        let original_text = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let file: toml::Table =
            toml::from_str(&original_text).map_err(|e| format!("{}: {e}", path.display()))?;
        let params = file.get("params").and_then(|p| p.as_table());
        let color_at = |v: Option<&toml::Value>| v.and_then(|v| parse_color(v.as_str()?).ok());

        let color = match &self.table {
            Some(table) => color_at(params.and_then(|p| p.get(table)?.get(&self.name))),
            None => {
                let link = file
                    .get("inputs")
                    .and_then(|i| i.get(&self.name))
                    .and_then(|v| v.as_str())
                    .and_then(|s| Reference::parse(s).ok());
                link.and_then(|r| linked_color(&self.dir, registry, &r))
                    .or_else(|| color_at(params.and_then(|p| p.get(&self.name))))
                    .or_else(|| self.example(registry, &file))
            }
        };
        Ok(Loaded {
            color: color.unwrap_or(Rgba([255, 255, 255, 255])),
            original_text,
            swatches: swatches(&self.dir, registry),
        })
    }

    fn example(&self, registry: &Registry, file: &toml::Table) -> Option<Rgba<u8>> {
        let kind = file.get("type")?.as_str()?;
        let p = registry.spec(kind)?.param(&self.name)?;
        let literal = p.default.unwrap_or(p.example);
        parse_color(literal.trim_matches('"')).ok()
    }

    /// Writes `color` (a typed color replaces any link).
    pub fn write(&self, color: Rgba<u8>) -> Result<(), String> {
        let hex = toml::Value::String(to_hex(color));
        let result = match &self.table {
            Some(table) => ops::set_entry(&self.dir, &self.node, table, &self.name, &hex),
            None => ops::set_param(&self.dir, &self.node, &self.name, &hex),
        };
        result.map(|_| ()).map_err(|e| e.to_string())
    }

    /// Puts the node file back exactly as it was.
    pub fn restore(&self, text: &str) -> Result<(), String> {
        std::fs::write(self.path(), text).map_err(|e| e.to_string())
    }
}

/// The value of a color output, by rendering the project (headless).
fn linked_color(dir: &Path, registry: &Registry, source: &Reference) -> Option<Rgba<u8>> {
    let graph = load(dir, registry).ok()?;
    match evaluate(&graph, registry, &mut NullHost)
        .ok()?
        .get(source)?
    {
        Value::Color(c) => Some(*c),
        _ => None,
    }
}

fn swatches(dir: &Path, registry: &Registry) -> Vec<(String, Rgba<u8>)> {
    let Ok(project) = inspect(dir, registry) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for node in project.nodes.values() {
        for p in node.spec.iter().flat_map(|s| &s.params) {
            if p.kind != ParamKind::Colors {
                continue;
            }
            let table = node.params.get(p.name).and_then(|t| t.as_table());
            for (name, value) in table.into_iter().flatten() {
                if let Some(c) = value.as_str().and_then(|s| parse_color(s).ok()) {
                    out.push((format!("{}.{name}", node.id), c));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/palette");
        for entry in std::fs::read_dir(example).unwrap() {
            let path = entry.unwrap().path();
            std::fs::copy(&path, tmp.path().join(path.file_name().unwrap())).unwrap();
        }
        tmp
    }

    fn target(dir: &Path, node: &str, name: &str, table: Option<&str>) -> Target {
        Target {
            dir: dir.to_path_buf(),
            node: node.into(),
            name: name.into(),
            table: table.map(String::from),
        }
    }

    #[test]
    fn loads_palette_typed_and_linked_colors() {
        let tmp = project();
        let registry = conduit_nodes::registry();
        let accent = target(tmp.path(), "brand", "accent", Some("colors"));
        let loaded = accent.load(&registry).unwrap();
        assert_eq!(to_hex(loaded.color), "#e76f51");
        assert_eq!(loaded.swatches.len(), 5);
        assert_eq!(loaded.swatches[0].0, "brand.primary");

        // `square.color` is linked to `accent_dark`, which darkens brand.accent.
        let square = target(tmp.path(), "square", "color", None);
        assert_eq!(to_hex(square.load(&registry).unwrap().color), "#cf401d");
    }

    #[test]
    fn writes_and_restores() {
        let tmp = project();
        let registry = conduit_nodes::registry();
        let square = target(tmp.path(), "square", "color", None);
        let loaded = square.load(&registry).unwrap();
        square.write(Rgba([1, 2, 3, 255])).unwrap();
        let text = std::fs::read_to_string(square.path()).unwrap();
        assert!(text.contains("color = \"#010203\""), "{text}");
        assert!(
            !text.contains("accent_dark"),
            "typing a color replaces the link"
        );
        square.restore(&loaded.original_text).unwrap();
        assert_eq!(
            std::fs::read_to_string(square.path()).unwrap(),
            loaded.original_text
        );

        let accent = target(tmp.path(), "brand", "accent", Some("colors"));
        accent.write(Rgba([0, 0, 0, 128])).unwrap();
        let text = std::fs::read_to_string(accent.path()).unwrap();
        assert!(text.contains("accent = \"#00000080\""), "{text}");
    }
}
