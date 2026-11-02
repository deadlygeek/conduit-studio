//! The file picker: `⏎` on a file param (e.g. `image.path`) lists the project's files with
//! the param's extensions, previewed in place (see `live.rs`).

use std::path::Path;

use conduit_core::{FontData, ParamKind};

use super::{App, Pick, Prompt, Row};

/// Folders deeper than this aren't searched.
const MAX_DEPTH: usize = 8;
/// Stop listing after this many files.
const MAX_FILES: usize = 5_000;

impl App {
    /// `⏎` on a file param: open the file picker on the current file. With no matching
    /// files in the project, opens the usual prompt instead.
    pub(super) fn open_file_picker(&mut self) -> bool {
        let Some(Row::Param { name, .. }) = self.current_row().cloned() else {
            return false;
        };
        let extensions = match self.param_spec(&name).map(|p| &p.kind) {
            Some(ParamKind::File { extensions }) => *extensions,
            _ => return false,
        };
        let current = self.text_value(&name);
        let paths = project_files(&self.dir, extensions);
        if paths.is_empty() {
            self.open_prompt(Prompt::Param { name }, current.unwrap_or_default());
            self.message = Some(format!(
                "no .{} files in the project folder; type a path",
                extensions.join("/.")
            ));
            return true;
        }
        self.files = paths
            .into_iter()
            .map(|path| {
                let mut notes = vec![describe(&self.dir.join(&path))];
                if current.as_deref() == Some(path.as_str()) {
                    notes.push("current".into());
                }
                let label = format!("{path}  {}", notes.join(" · "));
                (path, label)
            })
            .collect();
        self.open_live_pick(Pick::File { param: name }, current.as_deref());
        true
    }
}

/// `480×320 · 8 KB` for an image, `Droid Sans · 190 KB` for a font, else just the size.
fn describe(path: &Path) -> String {
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let size = match size {
        0..1_000 => format!("{size} B"),
        1_000..1_000_000 => format!("{} KB", size / 1_000),
        _ => format!("{:.1} MB", size as f64 / 1e6),
    };
    if let Ok((w, h)) = conduit_core::image::image_dimensions(path) {
        return format!("{w}×{h} · {size}");
    }
    let is_font = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        conduit_nodes::font::EXTENSIONS.contains(&e.to_ascii_lowercase().as_str())
    });
    if is_font
        && let Ok(data) = std::fs::read(path)
        && let Ok(families) = conduit_nodes::font::families(&FontData::new("", data))
    {
        return format!("{} · {size}", families.join(", "));
    }
    size
}

/// Files under `dir` with one of `extensions` (case doesn't matter), as `/`-separated paths
/// relative to it, sorted. Hidden files and folders, and the top-level `out` folder (saved
/// renders), are skipped.
pub(super) fn project_files(dir: &Path, extensions: &[&str]) -> Vec<String> {
    let mut found = Vec::new();
    walk(dir, &mut Vec::new(), extensions, &mut found);
    found.sort();
    found
}

fn walk(dir: &Path, prefix: &mut Vec<String>, extensions: &[&str], found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if found.len() >= MAX_FILES {
            return;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name.starts_with('.') || (prefix.is_empty() && name == "out") {
            continue;
        }
        let Ok(ty) = entry.file_type() else { continue };
        if ty.is_dir() {
            if prefix.len() < MAX_DEPTH {
                prefix.push(name);
                walk(&entry.path(), prefix, extensions, found);
                prefix.pop();
            }
        } else if Path::new(&name)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| extensions.iter().any(|x| x.eq_ignore_ascii_case(e)))
        {
            let mut path = prefix.join("/");
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(&name);
            found.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_matching_files_recursively() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for path in [
            "a.png",
            "notes.txt",
            "assets/images/Cat.JPG",
            "assets/b.webp",
            ".hidden/x.png",
            "assets/.y.png",
            "out/render.png",
            "sub/out/kept.png",
        ] {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }
        assert_eq!(
            project_files(root, &["png", "jpg", "webp"]),
            [
                "a.png",
                "assets/b.webp",
                "assets/images/Cat.JPG",
                "sub/out/kept.png"
            ]
        );
    }
}
