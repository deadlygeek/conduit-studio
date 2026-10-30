//! Built-in node kinds. To add one: create a module implementing `conduit_core::Node`
//! and register it in [`register_all`].

mod composite;
mod position;
mod preview;
mod rect;

#[cfg(test)]
mod test_util;

use conduit_core::Registry;

pub use composite::Composite;
pub use position::Position;
pub use preview::Preview;
pub use rect::Rect;

/// Largest width/height a node may create, to catch typos before allocating gigabytes.
pub const MAX_DIMENSION: u32 = 16_384;

pub fn register_all(registry: &mut Registry) {
    registry.register(Rect);
    registry.register(Position);
    registry.register(Composite);
    registry.register(Preview);
}

/// A registry containing every built-in node.
pub fn registry() -> Registry {
    let mut registry = Registry::new();
    register_all(&mut registry);
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keeps each node's declared `ParamSpec`s in sync with the serde struct that
    /// actually validates its params.
    #[test]
    fn param_specs_match_validation() {
        for spec in registry().specs() {
            let node = registry().get(spec.kind).unwrap().spec();
            let check = |params: toml::Table| {
                registry()
                    .get(spec.kind)
                    .unwrap()
                    .check_params(&params)
                    .unwrap_or_else(|e| panic!("{}: {e}\nparams: {params}", spec.kind));
            };

            // A fresh template is valid.
            let template: toml::Table = toml::from_str(&node.template()).unwrap();
            check(
                template
                    .get("params")
                    .and_then(|p| p.as_table())
                    .cloned()
                    .unwrap_or_default(),
            );

            // Every declared param (including optional ones) is accepted.
            let mut all = toml::Table::new();
            for p in &node.params {
                let literal = p.default.unwrap_or(p.example);
                let value: toml::Table = toml::from_str(&format!("v = {literal}")).unwrap();
                all.insert(p.name.to_string(), value["v"].clone());
            }
            check(all);
        }
    }
}
