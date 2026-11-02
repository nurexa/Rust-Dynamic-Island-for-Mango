//! Wallpaper settings: fit, speed, frame rate and quality, for all wallpapers or just
//! this one. The island only draws and sends clicks; `wall opt` (wall/wall) does the
//! work and rewrites the state file, then pokes `island wall` to redraw this.
use super::{Ctx, PAD, PANEL_W};
use crate::{
    pen::{Action, Pen, Region},
    settings::home,
};
use pango::Weight;
use serde::Deserialize;
use std::{env, fs, path::PathBuf};

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Now {
    pub name: String,
    pub fit: String,
    pub speed: f64,
    pub fps: String,
    pub quality: String,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub struct Wall {
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub busy: bool,
    pub now: Now,
}

impl Wall {
    /// The setting as the chips spell it, so a chip is lit when its value matches.
    pub fn get(&self, key: &str) -> String {
        match key {
            "fit" => self.now.fit.clone(),
            "speed" => self.now.speed.to_string(),
            "fps" => self.now.fps.clone(),
            _ => self.now.quality.clone(),
        }
    }
}

fn path() -> PathBuf {
    env::var_os("XDG_STATE_HOME").map_or(home().join(".local/state"), PathBuf::from).join("island/wallpaper.json")
}

/// None when `wall` has never run, or the file is unreadable: the module then draws nothing.
pub fn load() -> Option<Wall> {
    parse(&fs::read_to_string(path()).ok()?)
}

fn parse(src: &str) -> Option<Wall> {
    serde_json::from_str(src).ok()
}

/// (label, wall opt key, [(chip label, value)])
pub type Row = (&'static str, &'static str, &'static [(&'static str, &'static str)]);
pub const ROWS: &[Row] = &[
    ("Fit", "fit", &[("Fill", "fill"), ("Bars", "fit"), ("Stretch", "stretch")]),
    ("Speed", "speed", &[("0.5×", "0.5"), ("0.75×", "0.75"), ("1×", "1"), ("1.25×", "1.25"), ("1.5×", "1.5"), ("2×", "2")]),
    ("FPS", "fps", &[("Source", "source"), ("24", "24"), ("30", "30"), ("60", "60")]),
    ("Quality", "quality", &[("Low", "low"), ("Medium", "balanced"), ("High", "high"), ("Max", "max")]),
];
const TIPS: &[(&str, &str)] = &[
    ("fit", "Fill crops to the screen, Bars keeps the whole picture"),
    ("speed", "Playback speed"),
    ("fps", "Frame rate of the cached copy; Source keeps the video's own"),
    ("quality", "Higher is sharper and a bigger file"),
];

const H: f64 = 26.0;
const GAP: f64 = 6.0;

/// One chip at x, returns its width. A click runs `<wall opt> <key> <value>`.
#[allow(clippy::too_many_arguments)]
fn chip(p: &mut Pen, base: &str, tip: &str, (key, value): (&str, &str), label: &str, on: bool, x: f64, y: f64) -> f64 {
    let pal = p.pal;
    let w = p.width(label, 12.0, Weight::Medium) + 18.0;
    let r = (x, y, w, H);
    p.fill(r, H / 2.0, if on { pal.accent } else { pal.fg }, if on { 0.22 } else { 0.07 });
    p.text(label, 12.0, Weight::Medium, if on { pal.accent } else { pal.fg }, x + 9.0, y + H / 2.0, -1.0);
    p.regions.push(Region::new(r, H / 2.0, Action::Sh(format!("{base} {key} {value}"))).tip(tip));
    w
}

pub fn panel(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let base = &c.cfg.commands.wallpaper_opt;
    let (Some(wall), false) = (&c.m.wall, base.is_empty()) else { return y };
    let pal = p.pal;
    // Title: which wallpaper this edits, and whether the change applies to all or only it.
    let cy = y + H / 2.0;
    let name = wall.now.name.rsplit_once('.').map_or(wall.now.name.as_str(), |(s, _)| s);
    let scope: [(&str, &str); 2] = [("All", "all"), ("This one", "this")];
    let widths: Vec<f64> = scope.iter().map(|(l, _)| p.width(l, 12.0, Weight::Medium) + 18.0).collect();
    let mut x = PANEL_W - PAD + 6.0 - widths.iter().sum::<f64>() - GAP;
    let title_w = x - PAD - 8.0;
    let shown = if wall.busy { "Optimizing…" } else { name };
    p.text(shown, 12.5, Weight::Semibold, if wall.busy { pal.accent } else { pal.fg }, PAD, cy, title_w);
    for (&(label, value), w) in scope.iter().zip(&widths) {
        p.fill((x, y, *w, H), H / 2.0, if wall.scope == value { pal.accent } else { pal.fg }, if wall.scope == value { 0.22 } else { 0.07 });
        p.text(label, 12.0, Weight::Medium, if wall.scope == value { pal.accent } else { pal.fg }, x + 9.0, cy, -1.0);
        p.regions.push(Region::new((x, y, *w, H), H / 2.0, Action::Sh(format!("{base} scope {value}")))
            .right(Action::Sh(format!("{base} reset")))
            .tip("Change all wallpapers, or only this one · right-click: drop this one's own settings"));
        x += w + GAP;
    }
    let mut y = y + H + 8.0;
    for &(label, key, chips) in ROWS {
        let cy = y + H / 2.0;
        p.text(label, 12.0, Weight::Medium, pal.muted, PAD, cy, -1.0);
        let tip = TIPS.iter().find(|(k, _)| *k == key).map_or("", |(_, t)| *t);
        let mut x = PAD + 62.0;
        let current = wall.get(key);
        for &(text, value) in chips {
            x += chip(p, base, tip, (key, value), text, current == value, x, y) + GAP;
        }
        y += H + 4.0;
    }
    y + 6.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = r#"{"scope":"this","global":{},"per":{},"busy":true,
        "now":{"name":"a.mp4","fit":"fit","speed":1.25,"fps":"source","quality":"high"},"extra":1}"#;

    #[test]
    fn reads_what_wall_writes_and_matches_chip_values() {
        let w = parse(FILE).unwrap();
        assert!(w.busy && w.scope == "this");
        // Every row's current value must be spelled the way its chips spell it.
        for &(_, key, chips) in ROWS {
            assert!(chips.iter().any(|(_, v)| *v == w.get(key)), "{key} = {} has no chip", w.get(key));
        }
        assert_eq!(Wall { now: Now { speed: 1.0, ..w.now.clone() }, ..w }.get("speed"), "1");
    }

    #[test]
    fn a_wallpaper_less_or_broken_file_hides_the_module() {
        assert!(parse("{}").is_none());
        assert!(parse(r#"{"now":{}}"#).is_none());
        assert!(parse("not json").is_none());
    }
}
