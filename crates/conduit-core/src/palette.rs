//! Reading palettes from pasted text: a Coolors link, a list of hex codes, Coolors' CSS or
//! JSON exports (which carry names), Lospec `.hex` files, or GIMP `.gpl` palettes.

use image::Rgba;

use crate::color::parse_color;

/// The colors found in `text`, with a name for each: the one given in the text (made
/// into a valid id) or `c1`, `c2`, …. Names are unique. Errors if there are no colors.
pub fn parse_palette(text: &str) -> Result<Vec<(String, Rgba<u8>)>, String> {
    let found = if text.trim_start().starts_with("GIMP Palette") {
        parse_gpl(text)
    } else if text.contains("coolors.co/") {
        parse_coolors_url(text)
    } else {
        parse_hex_list(text)
    };
    if found.is_empty() {
        return Err("no colors found (paste a Coolors link or hex codes like #264653)".into());
    }
    Ok(name_colors(found))
}

/// `https://coolors.co/264653-2a9d8f-e9c46a` (also `/palette/…`): the dash-separated hex
/// codes in the last path segment.
fn parse_coolors_url(text: &str) -> Vec<(Option<String>, Rgba<u8>)> {
    let path = text.trim().split(['?', '#']).next().unwrap_or_default();
    let segment = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    segment
        .split('-')
        .filter_map(|hex| parse_color(&format!("#{hex}")).ok())
        .map(|c| (None, c))
        .collect()
}

/// `R G B name` lines after the `GIMP Palette` header.
fn parse_gpl(text: &str) -> Vec<(Option<String>, Rgba<u8>)> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let mut channel = || words.next()?.parse::<u8>().ok();
            let (r, g, b) = (channel()?, channel()?, channel()?);
            let name: Vec<&str> = words.collect();
            let name = (!name.is_empty() && name != ["Untitled"]).then(|| name.join(" "));
            Some((name, Rgba([r, g, b, 255])))
        })
        .collect()
}

/// Every 6- or 8-digit hex code (with or without `#`). A name directly before a code on
/// the same line is kept: `--charcoal: #264653;`, `"Charcoal":"264653"`, `accent = #e76f51`.
fn parse_hex_list(text: &str) -> Vec<(Option<String>, Rgba<u8>)> {
    let mut found = Vec::new();
    let mut gap_start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if !c.is_ascii_alphanumeric() {
            continue;
        }
        let mut end = start + c.len_utf8();
        while let Some(&(i, c)) = chars.peek() {
            if !c.is_ascii_alphanumeric() {
                break;
            }
            end = i + c.len_utf8();
            chars.next();
        }
        let word = &text[start..end];
        if !matches!(word.len(), 6 | 8) || !word.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let Ok(color) = parse_color(&format!("#{word}")) else {
            continue;
        };
        found.push((name_before(&text[gap_start..start]), color));
        gap_start = end;
    }
    found
}

/// The name at the end of the text between two colors, ignoring separators like `: " = #`.
fn name_before(gap: &str) -> Option<String> {
    let line = gap.rsplit('\n').next().unwrap_or_default();
    let line = line.trim_end_matches(|c: char| ":\"'=#,;( \t".contains(c));
    let start = line
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || matches!(c, '_' | '-' | ' '))
        .last()
        .map_or(line.len(), |(i, _)| i);
    let name = line[start..].trim().trim_matches('-').trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Turns names into unique ids (`Persian green` → `persian_green`), filling gaps with
/// `c<position>`.
fn name_colors(found: Vec<(Option<String>, Rgba<u8>)>) -> Vec<(String, Rgba<u8>)> {
    let mut out: Vec<(String, Rgba<u8>)> = Vec::new();
    for (i, (name, color)) in found.into_iter().enumerate() {
        let base = name
            .map(|n| to_id(&n))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("c{}", i + 1));
        let mut name = base.clone();
        let mut n = 2;
        while out.iter().any(|(existing, _)| *existing == name) {
            name = format!("{base}_{n}");
            n += 1;
        }
        out.push((name, color));
    }
    out
}

fn to_id(name: &str) -> String {
    let mut id = String::new();
    for c in name.trim().chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            id.push(c);
        } else if !id.ends_with('_') {
            id.push('_');
        }
    }
    id.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::to_hex;

    fn parsed(text: &str) -> Vec<(String, String)> {
        parse_palette(text)
            .unwrap()
            .into_iter()
            .map(|(name, c)| (name, to_hex(c)))
            .collect()
    }

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn coolors_links() {
        let expected = pairs(&[
            ("c1", "#264653"),
            ("c2", "#2a9d8f"),
            ("c3", "#e9c46a"),
            ("c4", "#f4a261"),
            ("c5", "#e76f51"),
        ]);
        for url in [
            "https://coolors.co/264653-2a9d8f-e9c46a-f4a261-e76f51",
            "  https://coolors.co/palette/264653-2a9d8f-e9c46a-f4a261-e76f51/  ",
            "coolors.co/264653-2a9d8f-e9c46a-f4a261-e76f51?ref=x",
        ] {
            assert_eq!(parsed(url), expected, "{url}");
        }
    }

    #[test]
    fn hex_lists() {
        let expected = pairs(&[("c1", "#264653"), ("c2", "#2a9d8f"), ("c3", "#e9c46a80")]);
        assert_eq!(parsed("#264653 #2a9d8f, #e9c46a80"), expected);
        assert_eq!(
            parsed("264653\n2a9d8f\ne9c46a80\n"),
            expected,
            "Lospec .hex"
        );
        assert_eq!(parsed(r#"["264653","2a9d8f","e9c46a80"]"#), expected);
    }

    #[test]
    fn named_exports() {
        let css = "/* CSS HEX */\n--charcoal: #264653ff;\n--persian-green: #2a9d8fff;\n";
        assert_eq!(
            parsed(css),
            pairs(&[("charcoal", "#264653"), ("persian_green", "#2a9d8f")])
        );
        let json = r#"{"Charcoal":"264653","Persian Green":"2a9d8f"}"#;
        assert_eq!(
            parsed(json),
            pairs(&[("charcoal", "#264653"), ("persian_green", "#2a9d8f")])
        );
        let toml = "accent = \"#e76f51\"\naccent = \"#000000\"";
        assert_eq!(
            parsed(toml),
            pairs(&[("accent", "#e76f51"), ("accent_2", "#000000")])
        );
    }

    #[test]
    fn gimp_palette() {
        let gpl = "GIMP Palette\nName: Test\nColumns: 4\n#\n 38  70  83\tCharcoal\n255 255 255 Untitled\n";
        assert_eq!(
            parsed(gpl),
            pairs(&[("charcoal", "#264653"), ("c2", "#ffffff")])
        );
    }

    #[test]
    fn nothing_found() {
        assert!(parse_palette("hello world").is_err());
        assert!(parse_palette("#12345").is_err());
        assert!(parse_palette("").is_err());
    }
}
