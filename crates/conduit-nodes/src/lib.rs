//! Built-in node kinds. To add one: create a module implementing `conduit_core::Node`
//! and register it in [`register_all`].

mod color;
mod composite;
mod duplicate;
mod effect;
pub mod font;
mod hsl;
pub mod image;
mod int;
mod invert;
mod palette;
mod preview;
mod project_file;
mod realize;
mod rect;
pub mod text;
mod tint;
mod transform;

#[cfg(test)]
mod test_util;

use conduit_core::Registry;

pub use color::ColorNode;
pub use composite::Composite;
pub use duplicate::Duplicate;
pub use font::Font;
pub use hsl::Hsl;
pub use image::Image;
pub use int::Int;
pub use invert::Invert;
pub use palette::Palette;
pub use preview::Preview;
pub use realize::Realize;
pub use rect::Rect;
pub use text::Text;
pub use tint::Tint;
pub use transform::Transform;

/// Largest width/height a node may create, to catch typos before allocating gigabytes.
pub const MAX_DIMENSION: u32 = 16_384;

pub fn register_all(registry: &mut Registry) {
    registry.register(Rect);
    registry.register(Image);
    registry.register(Transform);
    registry.register(Realize);
    registry.register(Hsl);
    registry.register(Tint);
    registry.register(Invert);
    registry.register(Duplicate);
    registry.register(Composite);
    registry.register(Preview);
    registry.register(Int);
    registry.register(ColorNode);
    registry.register(Palette);
    registry.register(Text::system());
    registry.register(Font);
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
        let registry = registry();
        for spec in registry.specs() {
            let node = registry.get(spec.kind).unwrap();
            let check = |params: toml::Table| {
                node.check_params(&params)
                    .unwrap_or_else(|e| panic!("{}: {e}\nparams: {params}", spec.kind));
            };

            // A fresh template is valid.
            let template: toml::Table = toml::from_str(&spec.template()).unwrap();
            check(
                template
                    .get("params")
                    .and_then(|p| p.as_table())
                    .cloned()
                    .unwrap_or_default(),
            );

            // Every declared param (including optional ones) is accepted.
            let mut all = toml::Table::new();
            for p in &spec.params {
                let literal = p.default.unwrap_or(p.example);
                let value = conduit_core::parse_literal(literal).unwrap();
                all.insert(p.name.to_string(), value);
            }
            check(all);
        }
    }
}
