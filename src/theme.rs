//! Colors come from one small file of tokens, re-read whenever it changes. Point
//! `palette` at anything that writes one; matugen, in the setup this was built on.
//!
//! Depth is worked out from those few colors, so any of them will do: `bg` (the island),
//! `card` (a tile) and `tint` (the tile that leads), each a step of contrast from the
//! last, not a fixed lightening, which vanishes on black and shouts on purple. What can be
//! pressed and what's cut in are veils of ink over those (see `Pen::raised`), and `raised`
//! is that veil over a card, kept here so text is checked against it.
use std::path::PathBuf;

pub type Rgb = (f64, f64, f64);

const BLACK: Rgb = (0.0, 0.0, 0.0);
const WHITE: Rgb = (1.0, 1.0, 1.0);

/// Above the island, as contrast against it: a card, the leading card.
const CARD: f64 = 1.22;
const TINT: f64 = 1.4;
/// How much ink a control's veil carries, dark and light: `Pen::raised` draws it.
pub const VEIL: (f64, f64) = (0.09, 0.07);
/// A dark island no lighter than this leaves white text 7:1 on the highest step.
const DARKEST_INK_BG: f64 = 0.04;

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub bg: Rgb,
    pub card: Rgb,
    pub tint: Rgb,
    pub raised: Rgb,
    /// The lit rim that separates one step from the next.
    pub edge: Rgb,
    pub dark: bool,
    pub fg: Rgb,
    pub muted: Rgb,
    pub accent: Rgb,
    pub on_accent: Rgb,
    pub error: Rgb,
}

/// `~` is the one bit of shell the config's palette path gets.
pub fn expand(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => crate::settings::home().join(rest),
        None => path.into(),
    }
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

/// The least mix of `from` toward `to` for which `far` holds, or all the way.
fn until(from: Rgb, to: Rgb, far: impl Fn(Rgb) -> bool) -> Rgb {
    if far(from) {
        return from;
    }
    let (mut short, mut enough) = (0.0, 1.0);
    for _ in 0..16 {
        let t = (short + enough) / 2.0;
        if far(mix(from, to, t)) { enough = t } else { short = t }
    }
    mix(from, to, enough)
}

fn step(from: Rgb, toward: Rgb, ratio: f64) -> Rgb {
    until(from, toward, |c| contrast(c, from) >= ratio)
}

/// Pull `c` toward black or white, whichever all of `on` leaves room for, until it
/// reads at `ratio` on every one of them.
fn readable(c: Rgb, on: &[Rgb], ratio: f64) -> Rgb {
    let worst = |c: Rgb| on.iter().map(|&bg| contrast(c, bg)).fold(f64::MAX, f64::min);
    let to = if worst(WHITE) >= worst(BLACK) { WHITE } else { BLACK };
    until(c, to, |c| worst(c) >= ratio)
}

fn build(surface: Rgb, fg: Rgb, muted: Rgb, accent: Rgb, on_accent: Rgb, error: Rgb) -> Palette {
    let dark = luminance(surface) < 0.5;
    let (bg, card, tint, edge);
    if dark {
        // Dark palettes sink most of the way to black: the island should read as
        // hardware, tinted just enough to belong to the wallpaper.
        bg = until(mix(surface, BLACK, 0.62), BLACK, |c| luminance(c) <= DARKEST_INK_BG);
        // The steps climb toward the accent's pale end, so they keep the palette's hue.
        let light = mix(accent, WHITE, 0.6);
        card = step(bg, light, CARD);
        tint = step(bg, mix(accent, WHITE, 0.15), TINT);
        edge = step(bg, light, 2.6);
    } else {
        // In the light there's no room above paper white, so the island steps back instead.
        bg = step(surface, BLACK, 1.12);
        card = mix(surface, WHITE, 0.5);
        tint = step(card, mix(accent, BLACK, 0.2), 1.16);
        edge = WHITE;
    }
    // Text is checked on a control too, whose veil is the text's own ink: settled in two passes.
    let veil = if dark { VEIL.0 } else { VEIL.1 };
    let mut fg = readable(fg, &[bg, card, tint], 7.0);
    let mut raised = mix(card, fg, veil);
    for _ in 0..2 {
        fg = readable(fg, &[bg, card, tint, raised], 7.0);
        raised = mix(card, fg, veil);
    }
    let all = [bg, card, tint, raised];
    let accent = readable(accent, &all, 3.0);
    Palette {
        bg,
        card,
        tint,
        raised,
        edge,
        dark,
        fg,
        muted: readable(muted, &all, 3.8),
        accent,
        on_accent: readable(on_accent, &[accent], 4.5),
        error: readable(error, &all, 3.0),
    }
}

pub fn parse(src: &str) -> Option<Palette> {
    let t = |key| token(src, key);
    Some(build(t("surface")?, t("on_surface")?, t("muted")?, t("accent")?, t("on_accent")?, t("error")?))
}

/// None on a file that doesn't parse, which is also what a read during a write
/// looks like: the caller keeps the palette it has.
pub fn reload(path: &str) -> Option<Palette> {
    parse(&std::fs::read_to_string(expand(path)).ok()?)
}

pub fn load(path: &str) -> Palette {
    let fallback = || parse(&std::fs::read_to_string(expand(path).parent()?.join("../tokens/colors.fallback.toml")).ok()?);
    reload(path).or_else(fallback).unwrap_or_else(|| {
        build(BLACK, WHITE, (0.6, 0.6, 0.6), (0.56, 0.81, 0.95), (0.0, 0.2, 0.3), (1.0, 0.7, 0.67))
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

    /// Any color anyone picks: 216 surfaces, each under 8 accents.
    #[test]
    fn every_color_keeps_its_layers_apart_and_its_text_readable() {
        let level = |i: usize| (i % 6) as f64 / 5.0;
        let cube: Vec<Rgb> = (0..216).map(|i| (level(i / 36), level(i / 6), level(i))).collect();
        let grey = (0.5, 0.5, 0.5);
        for &surface in &cube {
            for &accent in cube.iter().step_by(31).chain([&WHITE]) {
                let p = build(surface, grey, grey, accent, grey, grey);
                let at = format!("surface {surface:?}, accent {accent:?}");
                for on in [p.bg, p.card, p.tint, p.raised] {
                    assert!(contrast(p.fg, on) >= 6.9, "text, {at}");
                    assert!(contrast(p.muted, on) >= 3.7, "muted, {at}");
                    assert!(contrast(p.accent, on) >= 2.9, "accent, {at}");
                }
                assert!(contrast(p.on_accent, p.accent) >= 4.4, "on accent, {at}");
                assert!(contrast(p.card, p.bg) >= 1.08, "a card vanished, {at}");
                assert!(contrast(p.raised, p.card) >= 1.08, "a control vanished, {at}");
            }
        }
    }

    #[test]
    fn bad_tokens_are_rejected_not_guessed() {
        assert!(parse("surface = \"#zz\"").is_none());
        assert!(parse("").is_none());
    }
}
