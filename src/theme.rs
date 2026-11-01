//! Colors from the existing matugen pipeline. `theme/apply.sh` regenerates
//! theme/generated/colors.toml on every wallpaper change; that file is the
//! canonical token set, so we read it instead of adding a matugen template.
use std::path::PathBuf;

pub type Rgb = (f64, f64, f64);

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub bg: Rgb,
    pub fg: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
    pub on_accent: Rgb,
    pub error: Rgb,
}

pub fn dir() -> PathBuf {
    crate::settings::home().join(".config/theme/generated")
}

fn hex(s: &str) -> Option<Rgb> {
    let s = s.trim_start_matches('#');
    let n = |i: usize| Some(u8::from_str_radix(s.get(i..i + 2)?, 16).ok()? as f64 / 255.0);
    Some((n(0)?, n(2)?, n(4)?))
}

fn token(src: &str, key: &str) -> Option<Rgb> {
    let line = src.lines().find(|l| l.split_whitespace().next() == Some(key))?;
    hex(line.split('"').nth(1)?)
}

fn luminance((r, g, b): Rgb) -> f64 {
    let f = |c: f64| if c <= 0.03928 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
}

fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

pub fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t)
}

/// Pull `c` toward black or white, whichever the background leaves room for,
/// until it reads at `ratio`.
fn readable(c: Rgb, bg: Rgb, ratio: f64) -> Rgb {
    let to = if luminance(bg) < 0.5 { (1.0, 1.0, 1.0) } else { (0.0, 0.0, 0.0) };
    (0..=20).map(|i| mix(c, to, i as f64 / 20.0)).find(|&c| contrast(c, bg) >= ratio).unwrap_or(to)
}

pub fn parse(src: &str) -> Option<Palette> {
    let surface = token(src, "surface")?;
    let dark = luminance(surface) < 0.5;
    // Dark palettes sink most of the way to black: the island should read as
    // hardware, tinted just enough to belong to the wallpaper.
    let bg = if dark { mix(surface, (0.0, 0.0, 0.0), 0.62) } else { surface };
    let accent = readable(token(src, "accent")?, bg, 3.0);
    Some(Palette {
        bg,
        fg: readable(token(src, "on_surface")?, bg, 7.0),
        muted: readable(token(src, "muted")?, bg, 3.5),
        accent,
        on_accent: readable(token(src, "on_accent")?, accent, 4.5),
        error: readable(token(src, "error")?, bg, 3.0),
    })
}

/// None on a file that doesn't parse, which is also what a read during a write
/// looks like: the caller keeps the palette it has.
pub fn reload() -> Option<Palette> {
    parse(&std::fs::read_to_string(dir().join("colors.toml")).ok()?)
}

pub fn load() -> Palette {
    let fallback = || parse(&std::fs::read_to_string(dir().join("../tokens/colors.fallback.toml")).ok()?);
    reload().or_else(fallback).unwrap_or(Palette {
            bg: (0.0, 0.0, 0.0),
            fg: (1.0, 1.0, 1.0),
            muted: (0.6, 0.6, 0.6),
            accent: (0.56, 0.81, 0.95),
            on_accent: (0.0, 0.2, 0.3),
            error: (1.0, 0.7, 0.67),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pal([s, o, a]: [&str; 3]) -> Palette {
        parse(&format!(
            "accent = \"{a}\"\non_accent = \"#003549\"\nsurface = \"{s}\"\non_surface = \"{o}\"\n\
             muted = \"#8a9297\"\nglow = \"#8fcff3aa\"\nerror = \"#ffb4ab\"\n"
        ))
        .unwrap()
    }

    #[test]
    fn real_tokens_parse_and_stay_readable_in_both_modes() {
        for t in [["#0f1417", "#dfe3e7", "#8fcff3"], ["#f6fafe", "#171c1f", "#0a6780"],
                  ["#101010", "#303030", "#202020"]] {
            let p = pal(t);
            assert!(contrast(p.fg, p.bg) >= 7.0);
            assert!(contrast(p.muted, p.bg) >= 3.5);
            assert!(contrast(p.accent, p.bg) >= 3.0);
            assert!(contrast(p.on_accent, p.accent) >= 4.5);
        }
        assert!(luminance(pal(["#0f1417", "#dfe3e7", "#8fcff3"]).bg) < 0.01, "dark stays near black");
        assert!(luminance(pal(["#f6fafe", "#171c1f", "#0a6780"]).bg) > 0.8, "light palette stays light");
    }

    #[test]
    fn bad_tokens_are_rejected_not_guessed() {
        assert!(parse("surface = \"#zz\"").is_none());
        assert!(parse("").is_none());
    }
}
