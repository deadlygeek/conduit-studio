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
