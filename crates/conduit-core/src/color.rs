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

#[cfg(test)]
mod tests {
    use super::*;

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
}
