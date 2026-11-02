//! The `path` param shared by nodes that read a file from the project folder.

use std::path::{Component, Path, PathBuf};

use conduit_core::{NodeError, NodeResult, parse_params};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Params {
    pub path: String,
}

impl Params {
    pub fn parse(params: &toml::Table) -> NodeResult<Self> {
        let p: Self = parse_params(params)?;
        check_path(&p.path)?;
        Ok(p)
    }

    /// The file a node with these params reads, for [`conduit_core::Node::files`].
    pub fn files(params: &toml::Table) -> Vec<PathBuf> {
        Self::parse(params)
            .map(|p| vec![PathBuf::from(p.path)])
            .unwrap_or_default()
    }
}

/// Paths stay inside the project folder, so a project can be moved or shared as a whole.
fn check_path(path: &str) -> NodeResult<()> {
    let inside = Path::new(path)
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if path.trim().is_empty() {
        Err(NodeError::new("`path` is empty"))
    } else if !inside {
        Err(NodeError::new(format!(
            "`path` must be relative to the project folder and stay inside it, got `{path}`"
        )))
    } else {
        Ok(())
    }
}
