use image::Rgba;
use serde::{Deserialize, Deserializer};

/// A color parameter, written in node files as a name (`"white"`) or hex (`"#ff8800"`, `"#ff880080"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(pub Rgba<u8>);

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        parse_color(&s).map(Color).map_err(serde::de::Error::custom)
    }
}

const NAMED: &[(&str, [u8; 4])] = &[
    ("transparent", [0, 0, 0, 0]),
    ("black", [0, 0, 0, 255]),
    ("white", [255, 255, 255, 255]),
    ("red", [255, 0, 0, 255]),
    ("green", [0, 128, 0, 255]),
    ("lime", [0, 255, 0, 255]),
    ("blue", [0, 0, 255, 255]),
    ("yellow", [255, 255, 0, 255]),
    ("cyan", [0, 255, 255, 255]),
    ("magenta", [255, 0, 255, 255]),
    ("orange", [255, 165, 0, 255]),
    ("purple", [128, 0, 128, 255]),
    ("gray", [128, 128, 128, 255]),
    ("grey", [128, 128, 128, 255]),
];

pub fn parse_color(s: &str) -> Result<Rgba<u8>, String> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex)
            .ok_or_else(|| format!("invalid hex color `{s}` (use #rrggbb or #rrggbbaa)"));
    }
    let lower = s.to_ascii_lowercase();
    NAMED
        .iter()
        .find(|(name, _)| *name == lower)
        .map(|(_, rgba)| Rgba(*rgba))
        .ok_or_else(|| format!("unknown color `{s}`"))
}

fn parse_hex(hex: &str) -> Option<Rgba<u8>> {
    if !(hex.len() == 6 || hex.len() == 8) || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    let alpha = if hex.len() == 8 { byte(6)? } else { 255 };
    Some(Rgba([byte(0)?, byte(2)?, byte(4)?, alpha]))
}

/// Formats a color as `#rrggbb`, or `#rrggbbaa` when it isn't opaque.
pub fn to_hex(c: Rgba<u8>) -> String {
    let [r, g, b, a] = c.0;
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

/// Hue (degrees, 0..360), saturation and lightness (0..1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsl {
    pub h: f64,
    pub s: f64,
    pub l: f64,
}

pub fn to_hsl(c: Rgba<u8>) -> Hsl {
    let [r, g, b] = [c[0], c[1], c[2]].map(|v| f64::from(v) / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return Hsl { h: 0.0, s: 0.0, l };
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    Hsl { h, s, l }
}

/// Converts back to RGB with the given alpha.
pub fn from_hsl(hsl: Hsl, alpha: u8) -> Rgba<u8> {
    let Hsl { h, s, l } = hsl;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (hp.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let byte = |v: f64| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    Rgba([byte(r), byte(g), byte(b), alpha])
}

/// Adjustments as found in most image editors: rotate the hue by `hue` degrees, move
/// saturation and lightness by `saturation` / `lightness` percentage points (-100..100),
/// and scale alpha to `alpha` percent.
pub fn adjust(c: Rgba<u8>, hue: f64, saturation: f64, lightness: f64, alpha: f64) -> Rgba<u8> {
    let hsl = to_hsl(c);
    let adjusted = Hsl {
        h: (hsl.h + hue).rem_euclid(360.0),
        s: (hsl.s + saturation / 100.0).clamp(0.0, 1.0),
        l: (hsl.l + lightness / 100.0).clamp(0.0, 1.0),
    };
    let a = (f64::from(c[3]) * (alpha / 100.0).clamp(0.0, 1.0)).round() as u8;
    if hue == 0.0 && saturation == 0.0 && lightness == 0.0 {
        // Avoid rounding drift through HSL when only alpha changes.
        return Rgba([c[0], c[1], c[2], a]);
    }
    from_hsl(adjusted, a)
}

/// How light `c` looks, from 0 (black) to 1 (white): Rec. 709 weights on the stored
/// (sRGB) values, as image editors' grayscale and tint effects use. Alpha is ignored.
pub fn luminance(c: Rgba<u8>) -> f64 {
    (0.2126 * f64::from(c[0]) + 0.7152 * f64::from(c[1]) + 0.0722 * f64::from(c[2])) / 255.0
}

/// One adjustable part of a color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Hue,
    Saturation,
    Lightness,
    Red,
    Green,
    Blue,
    Alpha,
}

impl Channel {
    pub const HSLA: [Channel; 4] = [
        Channel::Hue,
        Channel::Saturation,
        Channel::Lightness,
        Channel::Alpha,
    ];
    pub const RGBA: [Channel; 4] = [Channel::Red, Channel::Green, Channel::Blue, Channel::Alpha];

    pub fn letter(self) -> &'static str {
        match self {
            Channel::Hue => "H",
            Channel::Saturation => "S",
            Channel::Lightness => "L",
            Channel::Red => "R",
            Channel::Green => "G",
            Channel::Blue => "B",
            Channel::Alpha => "A",
        }
    }
}

/// A color being adjusted channel by channel, as color editors do. HSL is kept alongside
/// RGB so the hue and saturation survive passing through greys, white and black.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorModel {
    pub color: Rgba<u8>,
    pub hsl: Hsl,
}

impl ColorModel {
    pub fn new(color: Rgba<u8>) -> Self {
        Self {
            color,
            hsl: to_hsl(color),
        }
    }

    pub fn set(&mut self, color: Rgba<u8>) {
        *self = Self::new(color);
    }

    /// Where `ch` sits in its range, 0..=1.
    pub fn position(&self, ch: Channel) -> f64 {
        match ch {
            Channel::Hue => self.hsl.h / 360.0,
            Channel::Saturation => self.hsl.s,
            Channel::Lightness => self.hsl.l,
            Channel::Red => f64::from(self.color[0]) / 255.0,
            Channel::Green => f64::from(self.color[1]) / 255.0,
            Channel::Blue => f64::from(self.color[2]) / 255.0,
            Channel::Alpha => f64::from(self.color[3]) / 255.0,
        }
    }

    /// Sets `ch` from a position in its range (clamped to 0..=1).
    pub fn set_position(&mut self, ch: Channel, t: f64) {
        let t = t.clamp(0.0, 1.0);
        let byte = (t * 255.0).round() as u8;
        match ch {
            Channel::Hue => self.hsl.h = t * 360.0,
            Channel::Saturation => self.hsl.s = t,
            Channel::Lightness => self.hsl.l = t,
            Channel::Red => self.color[0] = byte,
            Channel::Green => self.color[1] = byte,
            Channel::Blue => self.color[2] = byte,
            Channel::Alpha => {
                self.color[3] = byte;
                return;
            }
        }
        match ch {
            Channel::Hue | Channel::Saturation | Channel::Lightness => {
                self.color = from_hsl(self.hsl, self.color[3]);
            }
            _ => self.hsl = to_hsl(self.color),
        }
    }

    /// The channel in display units: degrees, percent, or 0–255.
    pub fn value(&self, ch: Channel) -> i64 {
        let t = self.position(ch);
        match ch {
            Channel::Hue => (t * 360.0).round() as i64,
            Channel::Red | Channel::Green | Channel::Blue => (t * 255.0).round() as i64,
            _ => (t * 100.0).round() as i64,
        }
    }

    /// The channel formatted for display: `12°`, `76%`, `231`.
    pub fn value_text(&self, ch: Channel) -> String {
        let v = self.value(ch);
        match ch {
            Channel::Hue => format!("{v}°"),
            Channel::Red | Channel::Green | Channel::Blue => v.to_string(),
            _ => format!("{v}%"),
        }
    }

    /// Moves `ch` by `delta` display units. Hue wraps around; the rest stop at their ends.
    pub fn nudge(&mut self, ch: Channel, delta: i64) {
        let v = self.value(ch) + delta;
        match ch {
            Channel::Hue => {
                self.hsl.h = (self.hsl.h + delta as f64).rem_euclid(360.0);
                self.color = from_hsl(self.hsl, self.color[3]);
            }
            Channel::Red | Channel::Green | Channel::Blue => {
                self.set_position(ch, v as f64 / 255.0)
            }
            _ => self.set_position(ch, v as f64 / 100.0),
        }
    }

    /// The color this one would be with `ch` at position `t` (for drawing gradients).
    pub fn with(&self, ch: Channel, t: f64) -> Rgba<u8> {
        let mut m = *self;
        m.set_position(ch, t);
        m.color
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        assert_eq!(to_hex(Rgba([255, 136, 0, 255])), "#ff8800");
        assert_eq!(to_hex(Rgba([255, 136, 0, 128])), "#ff880080");
        for s in ["#264653", "#2a9d8f80"] {
            assert_eq!(to_hex(parse_color(s).unwrap()), s);
        }
    }

    #[test]
    fn hsl_round_trip() {
        for hex in [
            "#000000", "#ffffff", "#808080", "#ff0000", "#00ff00", "#0000ff", "#264653", "#e9c46a",
            "#e76f51", "#2a9d8f",
        ] {
            let c = parse_color(hex).unwrap();
            assert_eq!(from_hsl(to_hsl(c), 255), c, "{hex}");
        }
        let red = to_hsl(Rgba([255, 0, 0, 255]));
        assert_eq!((red.h, red.s, red.l), (0.0, 1.0, 0.5));
        let grey = to_hsl(Rgba([128, 128, 128, 255]));
        assert_eq!((grey.h, grey.s), (0.0, 0.0));
    }

    #[test]
    fn adjustments() {
        let red = Rgba([255, 0, 0, 255]);
        assert_eq!(adjust(red, 120.0, 0.0, 0.0, 100.0), Rgba([0, 255, 0, 255]));
        assert_eq!(adjust(red, -120.0, 0.0, 0.0, 100.0), Rgba([0, 0, 255, 255]));
        assert_eq!(
            adjust(red, 0.0, 0.0, 50.0, 100.0),
            Rgba([255, 255, 255, 255])
        );
        assert_eq!(adjust(red, 0.0, 0.0, -50.0, 100.0), Rgba([0, 0, 0, 255]));
        assert_eq!(
            adjust(red, 0.0, -100.0, 0.0, 100.0),
            Rgba([128, 128, 128, 255])
        );
        assert_eq!(adjust(red, 0.0, 0.0, 0.0, 50.0), Rgba([255, 0, 0, 128]));
        let c = Rgba([38, 70, 83, 200]);
        assert_eq!(
            adjust(c, 0.0, 0.0, 0.0, 100.0),
            c,
            "no-op keeps the color exactly"
        );
    }

    #[test]
    fn named_colors() {
        assert_eq!(parse_color("white"), Ok(Rgba([255, 255, 255, 255])));
        assert_eq!(parse_color(" Black "), Ok(Rgba([0, 0, 0, 255])));
        assert_eq!(parse_color("transparent"), Ok(Rgba([0, 0, 0, 0])));
    }

    #[test]
    fn hex_colors() {
        assert_eq!(parse_color("#ff8800"), Ok(Rgba([255, 136, 0, 255])));
        assert_eq!(parse_color("#FF880080"), Ok(Rgba([255, 136, 0, 128])));
    }

    #[test]
    fn invalid_colors() {
        assert!(parse_color("chartreuse-ish").is_err());
        assert!(parse_color("#fff").is_err());
        assert!(parse_color("#gg0000").is_err());
        assert!(parse_color("#ééé").is_err());
    }

    #[test]
    fn model_channels() {
        let mut m = ColorModel::new(Rgba([255, 0, 0, 255]));
        m.nudge(Channel::Hue, 120);
        assert_eq!(to_hex(m.color), "#00ff00");
        m.nudge(Channel::Hue, -180);
        assert_eq!(m.value(Channel::Hue), 300, "wraps");
        m.nudge(Channel::Lightness, 100);
        assert_eq!(to_hex(m.color), "#ffffff");
        m.nudge(Channel::Lightness, -50);
        assert_eq!(to_hex(m.color), "#ff00ff", "hue survives white");
        m.nudge(Channel::Alpha, -50);
        assert_eq!(
            (m.color[3], m.value_text(Channel::Alpha)),
            (128, "50%".into())
        );

        let mut m = ColorModel::new(Rgba([16, 32, 48, 255]));
        m.nudge(Channel::Red, 10);
        m.nudge(Channel::Blue, -50);
        m.nudge(Channel::Green, 500);
        assert_eq!(to_hex(m.color), "#1aff00");
        assert_eq!(m.value_text(Channel::Green), "255");
        assert_eq!(m.with(Channel::Red, 0.0), Rgba([0, 255, 0, 255]));
    }
}
