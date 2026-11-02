use std::fmt;
use std::sync::Arc;

use image::{Rgba, RgbaImage};

use crate::affine::{Affine, Filter};

/// The type of a value flowing along a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueType {
    Image,
    Layer,
    Number,
    Color,
    Font,
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
            ValueType::Font => "font",
        };
        f.write_str(name)
    }
}

/// An image plus where it goes in a parent canvas. The transform only means something
/// once a parent exists, so pixels are produced only when a layer is drawn (`composite`)
/// or realized; until then transforms just combine.
#[derive(Clone)]
pub struct Layer {
    pub image: Arc<RgbaImage>,
    /// Maps the image's pixel coordinates to the parent's.
    pub transform: Affine,
    /// How the image is resampled when the transform isn't a whole-pixel move.
    pub filter: Filter,
}

impl Layer {
    /// The image untransformed, at the parent's origin.
    pub fn new(image: Arc<RgbaImage>) -> Self {
        Self {
            image,
            transform: Affine::IDENTITY,
            filter: Filter::default(),
        }
    }

    /// The image moved by whole pixels.
    pub fn at(image: Arc<RgbaImage>, x: i64, y: i64) -> Self {
        Self {
            transform: Affine::translate(x as f64, y as f64),
            ..Self::new(image)
        }
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (w, h) = self.image.dimensions();
        match self.transform.whole_pixel_offset() {
            Some((x, y)) => write!(f, "{w}x{h} at {x},{y}"),
            None => {
                let m = &self.transform;
                write!(
                    f,
                    "{w}x{h} transformed [{:.3} {:.3} {:.3} {:.3} {:.2} {:.2}] {}",
                    m.a, m.b, m.c, m.d, m.e, m.f, self.filter
                )
            }
        }
    }
}

/// A font file's contents (TrueType / OpenType), e.g. from a `font` node.
#[derive(Clone)]
pub struct FontData {
    /// Where it came from, for messages (e.g. `assets/fonts/my_font.ttf`).
    pub name: String,
    pub data: Arc<Vec<u8>>,
    /// Identifies the contents: equal for the same bytes.
    pub hash: u64,
}

impl FontData {
    pub fn new(name: impl Into<String>, data: Vec<u8>) -> Self {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut h = DefaultHasher::new();
        data.hash(&mut h);
        Self {
            name: name.into(),
            data: Arc::new(data),
            hash: h.finish(),
        }
    }
}

#[derive(Clone)]
pub enum Value {
    Image(Arc<RgbaImage>),
    Layer(Layer),
    Number(f64),
    Color(Rgba<u8>),
    Font(FontData),
}

impl Value {
    pub fn ty(&self) -> ValueType {
        match self {
            Value::Image(_) => ValueType::Image,
            Value::Layer(_) => ValueType::Layer,
            Value::Number(_) => ValueType::Number,
            Value::Color(_) => ValueType::Color,
            Value::Font(_) => ValueType::Font,
        }
    }

    /// Converts this value to `target`, applying implicit coercions (image → untransformed
    /// layer).
    pub fn coerce(self, target: ValueType) -> Option<Value> {
        match (self, target) {
            (v, t) if v.ty() == t => Some(v),
            (Value::Image(image), ValueType::Layer) => Some(Value::Layer(Layer::new(image))),
            _ => None,
        }
    }
}

// Manual impl: deriving would dump every pixel.
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Image(img) => write!(f, "Image({}x{})", img.width(), img.height()),
            Value::Layer(l) => write!(f, "Layer({l})"),
            Value::Number(n) => write!(f, "Number({n})"),
            Value::Color(c) => write!(f, "Color({:?})", c.0),
            Value::Font(font) => write!(f, "Font({})", font.name),
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
        assert_eq!(layer.transform, Affine::IDENTITY);
        assert_eq!(layer.image.width(), 2);
    }

    #[test]
    fn incompatible_types_do_not_coerce() {
        assert!(Value::Number(1.0).coerce(ValueType::Image).is_none());
        assert!(!ValueType::Layer.coerces_to(ValueType::Image));
    }
}
