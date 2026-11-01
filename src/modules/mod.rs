//! The pill and the panel are lists of modules, in the order config.toml gives.
//! A module is a function that draws at a position and returns the next one, so
//! adding one is a new file here and one line in `PILL` or `PANEL`. MODDING.md
//! has a worked example.
use crate::{
    Model,
    icons::Icons,
    pen::{Content, Pen, View},
    settings::Config,
};

mod audio;
mod clock;
pub mod custom;
mod header;
mod media;
mod menu;
mod stats;
mod status;
mod workspaces;

pub const PILL_H: f64 = 32.0;
pub const PANEL_W: f64 = 560.0;
pub const MAX_PANEL_H: f64 = 680.0;
pub const MAX_COMPACT_W: f64 = 680.0;
const MIN_COMPACT_W: f64 = 168.0;
const PAD: f64 = 20.0;
const PILL_PAD: (f64, f64) = (10.0, 16.0);
const PILL_GAP: f64 = 9.0;

/// Everything a module can read.
pub struct Ctx<'a> {
    pub m: &'a Model,
    pub cfg: &'a Config,
    /// The output this island is on.
    pub mon: &'a str,
    pub pinned: bool,
    pub tucked: bool,
}

/// Draws starting at x, returns where it ended. Returning x unchanged means
/// nothing to show, and the module takes up no room.
type PillModule = fn(&mut Pen, &Ctx, f64) -> f64;
/// Draws starting at y, returns the y the next module starts at, spacing included.
type PanelModule = fn(&mut Pen, &Ctx, f64) -> f64;

const PILL: &[(&str, PillModule)] = &[
    ("media", media::pill),
    ("workspaces", workspaces::pill),
    ("network", status::offline_pill),
    ("clock", clock::pill),
];

const PANEL: &[(&str, PanelModule)] = &[
    ("header", header::panel),
    ("workspaces", workspaces::panel),
    ("media", media::panel),
    ("stats", stats::panel),
    ("audio", audio::panel),
    ("status", status::panel),
];

fn draw_pill(name: &str, p: &mut Pen, c: &Ctx, x: f64) -> f64 {
    if let Some(custom) = name.strip_prefix("custom:") {
        return custom::pill(p, c, custom, x);
    }
    PILL.iter().find(|(n, _)| *n == name).map_or(x, |(_, draw)| draw(p, c, x))
}

fn draw_panel(name: &str, p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    if let Some(custom) = name.strip_prefix("custom:") {
        return custom::panel(p, c, custom, y);
    }
    PANEL.iter().find(|(n, _)| *n == name).map_or(y, |(_, draw)| draw(p, c, y))
}

pub fn pill(m: &Model, icons: &mut Icons, cfg: &Config, mon: &str, scale: i32, flash: bool) -> Option<Content> {
    let (surf, mut p) = Pen::new(MAX_COMPACT_W, PILL_H, cfg, &m.pal, scale, icons)?;
    let c = Ctx { m, cfg, mon, pinned: false, tucked: false };
    if flash {
        if let Some(w) = audio::flash(&mut p, &c) {
            return Some(Content { surf, w, h: PILL_H, regions: Vec::new() });
        }
    }
    // Laid out twice when short of the minimum width: the second pass pushes the
    // last module to the far end, which is where the clock belongs.
    let mut spare = 0.0;
    let w = loop {
        let mut x = PILL_PAD.0;
        for (i, name) in cfg.pill.iter().enumerate() {
            if i + 1 == cfg.pill.len() {
                x += spare;
            }
            let end = draw_pill(name, &mut p, &c, x);
            x = if end > x { end + PILL_GAP } else { x };
        }
        let w = x - PILL_GAP + PILL_PAD.1;
        if spare > 0.0 || w >= MIN_COMPACT_W {
            break w.max(MIN_COMPACT_W);
        }
        spare = MIN_COMPACT_W - w;
        p.clear();
    };
    let regions = std::mem::take(&mut p.regions);
    Some(Content { surf, w, h: PILL_H, regions })
}

#[allow(clippy::too_many_arguments)]
pub fn panel(m: &Model, icons: &mut Icons, cfg: &Config, mon: &str, view: &View, scale: i32, pinned: bool, tucked: bool) -> Option<Content> {
    let (surf, mut p) = Pen::new(PANEL_W, MAX_PANEL_H, cfg, &m.pal, scale, icons)?;
    let c = Ctx { m, cfg, mon, pinned, tucked };
    let h = match view {
        View::Main => cfg.panel.iter().fold(16.0, |y, name| draw_panel(name, &mut p, &c, y)),
        View::Menu { title, entries, .. } => menu::view(&mut p, title, entries),
    };
    let regions = std::mem::take(&mut p.regions);
    Some(Content { surf, w: PANEL_W, h: h.min(MAX_PANEL_H), regions })
}
