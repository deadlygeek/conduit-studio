use std::fmt;
use std::sync::Arc;

use image::{Rgba, RgbaImage};

/// The type of a value flowing along a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueType {
    Image,
    Layer,
    Number,
    Color,
}

impl ValueType {
    /// Whether a value of type `self` can be fed into a port expecting `target`.
    pub fn coerces_to(self, target: ValueType) -> bool {
        self == target || matches!((self, target), (ValueType::Image, ValueType::Layer))
    }
}

impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            ValueType::Image => "image",
            ValueType::Layer => "layer",
            ValueType::Number => "number",
            ValueType::Color => "color",
        };
        f.write_str(name)
    }
}

/// An image placed at an offset, ready to be composited.
#[derive(Clone)]
pub struct Layer {
    pub image: Arc<RgbaImage>,
    pub x: i64,
    pub y: i64,
}

#[derive(Clone)]
pub enum Value {
    Image(Arc<RgbaImage>),
    Layer(Layer),
    Number(f64),
    Color(Rgba<u8>),
}

impl Value {
    pub fn ty(&self) -> ValueType {
        match self {
            Value::Image(_) => ValueType::Image,
            Value::Layer(_) => ValueType::Layer,
            Value::Number(_) => ValueType::Number,
            Value::Color(_) => ValueType::Color,
        }
    }

    /// Converts this value to `target`, applying implicit coercions (image → layer at 0,0).
    pub fn coerce(self, target: ValueType) -> Option<Value> {
        match (self, target) {
            (v, t) if v.ty() == t => Some(v),
            (Value::Image(image), ValueType::Layer) => {
                Some(Value::Layer(Layer { image, x: 0, y: 0 }))
            }
            _ => None,
        }
    }
}

// Manual impl: deriving would dump every pixel.
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Image(img) => write!(f, "Image({}x{})", img.width(), img.height()),
            Value::Layer(l) => write!(
                f,
                "Layer({}x{} @ {},{})",
                l.image.width(),
                l.image.height(),
                l.x,
                l.y
            ),
            Value::Number(n) => write!(f, "Number({n})"),
            Value::Color(c) => write!(f, "Color({:?})", c.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_coerces_to_layer_at_origin() {
        let img = Arc::new(RgbaImage::new(2, 3));
        let Some(Value::Layer(layer)) = Value::Image(img).coerce(ValueType::Layer) else {
            panic!("expected layer");
        };
        assert_eq!((layer.x, layer.y, layer.image.width()), (0, 0, 2));
    }

    #[test]
    fn incompatible_types_do_not_coerce() {
        assert!(Value::Number(1.0).coerce(ValueType::Image).is_none());
        assert!(!ValueType::Layer.coerces_to(ValueType::Image));
    }
}
