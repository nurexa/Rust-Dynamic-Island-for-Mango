//! The pill and the panel are lists of modules, in the order config.toml gives.
//! A module is a function that draws at a position and returns the next one, so
//! adding one is a new file here and one line in `PILL` or `PANEL`. MODDING.md
//! has a worked example.
use crate::{
    Model,
    icons::Icons,
    pen::{Content, Pen, View},
    settings::Config,
    style::{SPLIT_GAP, Style},
    viz::Look,
};

mod audio;
mod battery;
mod bluetooth;
mod brightness;
mod calendar;
mod claude;
mod clock;
pub mod custom;
mod header;
mod media;
mod menu;
mod nightlight;
mod notify;
mod palette;
mod privacy;
mod rail;
mod sidebar;
mod stats;
mod status;
mod timer;
pub mod wallpaper;
mod workspaces;

pub const PILL_H: f64 = 32.0;
pub const PANEL_W: f64 = 560.0;
pub const MAX_PANEL_H: f64 = 680.0;
pub const MAX_COMPACT_W: f64 = 680.0;
/// The most the split style's bubble adds.
pub const BUBBLE_W: f64 = 220.0;
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

impl Ctx<'_> {
    /// Whether to leave room for this visualizer.
    fn looks(&self, look: Look) -> bool {
        self.m.can_capture && self.cfg.visualizer.contains(&look)
    }
}

/// The column labels take beside a slider.
fn label_w(p: &Pen) -> f64 {
    if p.w < PANEL_W { 104.0 } else { 168.0 }
}

/// Draws starting at x, returns where it ended. Returning x unchanged means
/// nothing to show, and the module takes up no room.
type PillModule = fn(&mut Pen, &Ctx, f64) -> f64;
/// Draws starting at y, returns the y the next module starts at, spacing included.
type PanelModule = fn(&mut Pen, &Ctx, f64) -> f64;

const PILL: &[(&str, PillModule)] = &[
    ("media", media::pill),
    ("workspaces", workspaces::pill),
    ("claude", claude::pill),
    ("timer", timer::pill),
    ("network", status::offline_pill),
    ("privacy", privacy::pill),
    ("battery", battery::pill),
    ("clock", clock::pill),
];

const PANEL: &[(&str, PanelModule)] = &[
    ("header", header::panel),
    ("workspaces", workspaces::panel),
    ("media", media::panel),
    ("calendar", calendar::panel),
    ("timer", timer::panel),
    ("notifications", notify::panel),
    ("stats", stats::panel),
    ("audio", audio::panel),
    ("brightness", brightness::panel),
    ("nightlight", nightlight::panel),
    ("status", status::panel),
    ("bluetooth", bluetooth::panel),
    ("wallpaper", wallpaper::panel),
];

/// Every built-in module name, pill then panel, for `island doctor`.
pub fn names() -> (Vec<&'static str>, Vec<&'static str>, Vec<&'static str>) {
    (PILL.iter().map(|m| m.0).collect(), PANEL.iter().map(|m| m.0).collect(), sidebar::SECTIONS.iter().map(|m| m.0).collect())
}

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
    let style = m.style;
    let room = if style == Style::Split { MAX_COMPACT_W + BUBBLE_W } else { MAX_COMPACT_W };
    let (surf, mut p) = Pen::new(room, PILL_H, cfg, &m.pal, scale, icons)?;
    let c = Ctx { m, cfg, mon, pinned: false, tucked: false };
    if flash {
        let w = if m.flash_power { battery::flash(&mut p, &c) } else { audio::flash(&mut p, &c) };
        if let Some(w) = w {
            return Some(Content { surf, w, h: PILL_H, regions: Vec::new(), split: None, live: Vec::new() });
        }
    }
    let (bubble, main): (Vec<&str>, Vec<&str>) = cfg.pill.iter().map(String::as_str)
        .filter(|n| style.on_island(n)).partition(|n| style.in_bubble(n));
    let min = if matches!(style, Style::Minimal | Style::Frame | Style::Bar) { PILL_H } else { MIN_COMPACT_W };
    // Laid out twice when short of the minimum width: the second pass pushes the
    // last module to the far end, which is where the clock belongs.
    let mut spare = 0.0;
    let (main_w, empty) = loop {
        let mut x = PILL_PAD.0;
        for (i, name) in main.iter().enumerate() {
            if i + 1 == main.len() {
                x += spare;
            }
            let end = draw_pill(name, &mut p, &c, x);
            x = if end > x { end + PILL_GAP } else { x };
        }
        if x == PILL_PAD.0 + spare {
            break (min, true);
        }
        let w = x - PILL_GAP + PILL_PAD.1;
        if spare > 0.0 || w >= min {
            break (w.max(min), false);
        }
        spare = min - w;
        p.clear();
    };
    let (mut w, mut split) = (main_w, None);
    if !bubble.is_empty() {
        let at = main_w + SPLIT_GAP;
        let mut x = at + PILL_PAD.0;
        for name in &bubble {
            let end = draw_pill(name, &mut p, &c, x);
            x = if end > x { end + PILL_GAP } else { x };
        }
        if x > at + PILL_PAD.0 {
            w = (x - PILL_GAP + PILL_PAD.0).max(at + PILL_H);
            split = Some(at);
        }
    }
    // Nothing to say, and somewhere to draw back into: no height at all.
    let h = if empty && style == Style::Frame { 0.0 } else { PILL_H };
    let regions = std::mem::take(&mut p.regions);
    Some(Content { surf, w, h, regions, split, live: p.live })
}

/// The frame's rail, over the whole of its strip.
pub fn rail(m: &Model, icons: &mut Icons, cfg: &Config, mon: &str, (w, h): (f64, f64), scale: i32) -> Option<Content> {
    let (surf, mut p) = Pen::new(w, h, cfg, &m.pal, scale, icons)?;
    rail::rail(&mut p, &Ctx { m, cfg, mon, pinned: false, tucked: false }, h);
    let regions = std::mem::take(&mut p.regions);
    Some(Content { surf, w, h, regions, split: None, live: Vec::new() })
}

/// The sidebar at a scroll offset: what's in view, and the height of the whole of it.
pub fn sidebar(m: &Model, icons: &mut Icons, cfg: &Config, mon: &str, (w, room): (f64, f64), scroll: f64, scale: i32) -> Option<(Content, f64)> {
    let (surf, mut p) = Pen::new(w, room, cfg, &m.pal, scale, icons)?;
    p.cr.translate(0.0, -scroll.round());
    let total = sidebar::draw(&mut p, &Ctx { m, cfg, mon, pinned: false, tucked: false });
    for r in &mut p.regions {
        r.rect.1 -= scroll.round();
    }
    for l in &mut p.live {
        l.rect.1 -= scroll.round();
    }
    let h = total.min(room);
    let regions = std::mem::take(&mut p.regions);
    Some((Content { surf, w, h, regions, split: None, live: p.live }, total))
}

/// The bar's two ends.
pub fn bar(m: &Model, icons: &mut Icons, cfg: &Config, mon: &str, (w, h): (f64, f64), scale: i32) -> Option<Content> {
    let (surf, mut p) = Pen::new(w, h, cfg, &m.pal, scale, icons)?;
    rail::bar(&mut p, &Ctx { m, cfg, mon, pinned: false, tucked: false }, w);
    let regions = std::mem::take(&mut p.regions);
    Some(Content { surf, w, h, regions, split: None, live: Vec::new() })
}

#[allow(clippy::too_many_arguments)]
pub fn panel(m: &Model, icons: &mut Icons, cfg: &Config, mon: &str, view: &View, scale: i32, pinned: bool, tucked: bool) -> Option<Content> {
    let (surf, mut p) = Pen::new(PANEL_W, MAX_PANEL_H, cfg, &m.pal, scale, icons)?;
    let c = Ctx { m, cfg, mon, pinned, tucked };
    let h = match view {
        View::Main => cfg.panel.iter().fold(16.0, |y, name| draw_panel(name, &mut p, &c, y)),
        View::Menu { title, entries, .. } => menu::view(&mut p, title, entries),
        View::Palette => palette::view(&mut p, &c),
        View::Toast => notify::toast(&mut p, &c),
    };
    let regions = std::mem::take(&mut p.regions);
    let w = if *view == View::Toast { notify::TOAST_W } else { PANEL_W };
    Some(Content { surf, w, h: h.min(MAX_PANEL_H), regions, split: None, live: p.live })
}
