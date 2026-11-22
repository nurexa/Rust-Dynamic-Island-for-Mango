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
mod dash;
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
pub const MAX_PANEL_H: f64 = 900.0;
pub const MAX_COMPACT_W: f64 = 680.0;
/// The most the split style's bubble adds.
pub const BUBBLE_W: f64 = 220.0;
const MIN_COMPACT_W: f64 = 168.0;
const PAD: f64 = 20.0;
/// Even at both ends, so whatever shows alone sits in the middle.
const PILL_PAD: (f64, f64) = (13.0, 13.0);
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

/// How solid the layers on the island are: through glass they still show some of it.
fn layered(cfg: &Config, style: Style) -> f64 {
    0.4 + 0.6 * cfg.alpha(style)
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
    ("clock", dash::clock),
    ("weather", dash::weather),
    ("about", dash::about),
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

/// How far a card reaches past the text on it, across and down, and the air between two.
const CARD: (f64, f64) = (12.0, 10.0);
const CARD_GAP: f64 = 8.0;
pub const CARD_R: f64 = 18.0;

/// What a panel module sits on.
#[derive(Clone, Copy, PartialEq)]
enum Seat {
    /// The island itself: the header.
    Bare,
    /// The tile that leads: what's playing.
    Lead,
    Card,
    /// Shares one tile with its neighbours of the same kind.
    Levels,
    Links,
}

/// Where a module sits, the share of a row it asks for, and the spacing it leaves
/// under itself, which its tile takes back.
fn seat(name: &str) -> (Seat, f64, f64) {
    match name {
        "header" => (Seat::Bare, 1.0, 12.0),
        "media" => (Seat::Lead, 1.0, 16.0),
        "clock" => (Seat::Card, 0.22, 0.0),
        "calendar" => (Seat::Card, 0.48, 12.0),
        "stats" => (Seat::Card, 0.3, 0.0),
        "weather" => (Seat::Card, 0.42, 0.0),
        "about" => (Seat::Card, 0.58, 0.0),
        "workspaces" => (Seat::Card, 1.0, 14.0),
        "audio" | "brightness" | "nightlight" => (Seat::Levels, 1.0, 2.0),
        "status" | "bluetooth" => (Seat::Links, 1.0, 12.0),
        "timer" => (Seat::Card, 1.0, 8.0),
        "wallpaper" => (Seat::Card, 1.0, 6.0),
        "notifications" => (Seat::Card, 1.0, 12.0),
        _ => (Seat::Card, 1.0, 4.0),
    }
}

/// One module drawn into a column `w` wide whose content starts at `x`: modules lay
/// out between PAD and `p.w - PAD`, so they're moved and narrowed rather than told.
fn column(name: &str, p: &mut Pen, c: &Ctx, (x, w): (f64, f64), y: f64) -> f64 {
    let (full, dx, regions, live) = (p.w, x - PAD, p.regions.len(), p.live.len());
    p.w = w + 2.0 * PAD;
    p.cr.translate(dx, 0.0);
    let next = draw_panel(name, p, c, y);
    p.cr.translate(-dx, 0.0);
    p.w = full;
    p.regions[regions..].iter_mut().for_each(|r| r.rect.0 += dx);
    p.live[live..].iter_mut().for_each(|l| l.rect.0 += dx);
    next
}

/// A tile's modules stacked in it from `y`; where its content ends, or None for nothing to show.
fn tile(group: &[&str], p: &mut Pen, c: &Ctx, x: f64, w: f64, y: f64) -> Option<f64> {
    let mut end: Option<f64> = None;
    for name in group {
        let at = end.map_or(y, |e| e + 2.0);
        let next = column(name, p, c, (x, w), at);
        if next > at {
            end = Some(next - seat(name).2);
        }
    }
    end
}

/// The panel's modules in their order: each tile asks for a share of a row, rows fill
/// in order, and what's in a row grows to fill it at the height of its tallest.
fn layers(p: &mut Pen, c: &Ctx, mut y: f64) -> f64 {
    // Tiles: runs of Levels or Links share one.
    let mut tiles: Vec<(Seat, f64, Vec<&str>)> = Vec::new();
    for name in &c.cfg.panel {
        let (seat, share, _) = seat(name);
        match tiles.last_mut() {
            Some((s, _, group)) if *s == seat && matches!(seat, Seat::Levels | Seat::Links) => group.push(name),
            _ => tiles.push((seat, share, vec![name])),
        }
    }
    let left = PAD - CARD.0;
    let room = p.w - 2.0 * left;
    let mut i = 0;
    while i < tiles.len() {
        let mut row = vec![i];
        let mut sum = tiles[i].1;
        while tiles[i].0 != Seat::Bare && i + row.len() < tiles.len() {
            let next = &tiles[i + row.len()];
            if next.0 == Seat::Bare || sum + next.1 > 1.001 {
                break;
            }
            sum += next.1;
            row.push(i + row.len());
        }
        i += row.len();
        if tiles[row[0]].0 == Seat::Bare {
            if let Some(end) = tile(&tiles[row[0]].2, p, c, PAD, p.w - 2.0 * PAD, y) {
                y = end + CARD_GAP;
            }
            continue;
        }
        // Each tile drawn apart, so one with nothing to show can leave its share to the rest.
        let (regions, live) = (p.regions.len(), p.live.len());
        let drawn = loop {
            let total: f64 = row.iter().map(|&t| tiles[t].1).sum();
            let free = room - CARD_GAP * (row.len() - 1) as f64;
            let mut x = left;
            let mut drawn = Vec::new();
            for &t in &row {
                let w = free * tiles[t].1 / total;
                let marks = (p.regions.len(), p.live.len());
                p.cr.push_group();
                let end = tile(&tiles[t].2, p, c, x + CARD.0, w - 2.0 * CARD.0, y + CARD.1);
                let ink = p.cr.pop_group().ok();
                drawn.push((t, x, w, end, ink, marks, (p.regions.len(), p.live.len())));
                x += w + CARD_GAP;
            }
            if drawn.iter().all(|d| d.3.is_some()) {
                break drawn;
            }
            p.regions.truncate(regions);
            p.live.truncate(live);
            row.retain(|&t| drawn.iter().any(|d| d.0 == t && d.3.is_some()));
            if row.is_empty() {
                break Vec::new();
            }
        };
        let Some(bottom) = drawn.iter().filter_map(|d| d.3).reduce(f64::max) else { continue };
        let h = bottom + CARD.1 - y;
        for (t, x, w, end, ink, from, to) in drawn {
            let face = if tiles[t].0 == Seat::Lead { p.pal.tint } else { p.pal.card };
            p.surface((x, y, w, h), CARD_R, face);
            // What's shorter than its row sits in the middle of it.
            let dy = ((bottom - end.unwrap_or(bottom)) / 2.0).round();
            p.regions[from.0..to.0].iter_mut().for_each(|r| r.rect.1 += dy);
            p.live[from.1..to.1].iter_mut().for_each(|l| l.rect.1 += dy);
            if let Some(ink) = ink {
                let _ = p.cr.save();
                p.cr.translate(0.0, dy);
                if p.cr.set_source(&ink).is_ok() {
                    let _ = p.cr.paint();
                }
                let _ = p.cr.restore();
            }
        }
        y += h + CARD_GAP;
    }
    y - CARD_GAP + 2.0 + 8.0
}

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

pub fn pill(m: &Model, icons: &mut Icons, cfg: &Config, mon: &str, scale: crate::scale::Scale, flash: bool) -> Option<Content> {
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
    let min = if matches!(style, Style::Minimal | Style::Frame | Style::Bar | Style::Glass) { PILL_H } else { MIN_COMPACT_W };
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
pub fn rail(m: &Model, icons: &mut Icons, cfg: &Config, mon: &str, (w, h): (f64, f64), scale: crate::scale::Scale) -> Option<Content> {
    let (surf, mut p) = Pen::new(w, h, cfg, &m.pal, scale, icons)?;
    rail::rail(&mut p, &Ctx { m, cfg, mon, pinned: false, tucked: false }, h);
    let regions = std::mem::take(&mut p.regions);
    Some(Content { surf, w, h, regions, split: None, live: Vec::new() })
}

/// The sidebar at a scroll offset: what's in view, and the height of the whole of it.
pub fn sidebar(m: &Model, icons: &mut Icons, cfg: &Config, mon: &str, (w, room): (f64, f64), scroll: f64, scale: crate::scale::Scale) -> Option<(Content, f64)> {
    let (surf, mut p) = Pen::new(w, room, cfg, &m.pal, scale, icons)?;
    p.solid = layered(cfg, m.style);
    p.cr.translate(0.0, -scroll.round());
    let total = sidebar::draw(&mut p, &Ctx { m, cfg, mon, pinned: false, tucked: false });
    let h = total.min(room);
    crate::pen::scrolled(&mut p.regions, &mut p.live, scroll.round(), h);
    let regions = std::mem::take(&mut p.regions);
    Some((Content { surf, w, h, regions, split: None, live: p.live }, total))
}

/// The bar's two ends.
pub fn bar(m: &Model, icons: &mut Icons, cfg: &Config, mon: &str, (w, h): (f64, f64), scale: crate::scale::Scale) -> Option<Content> {
    let (surf, mut p) = Pen::new(w, h, cfg, &m.pal, scale, icons)?;
    let c = Ctx { m, cfg, mon, pinned: false, tucked: false };
    if m.style == Style::Glass {
        let top = crate::style::FLOAT;
        p.cr.translate(0.0, top);
        rail::glass(&mut p, &c, w);
        for r in &mut p.regions {
            r.rect.1 += top;
        }
    } else {
        rail::bar(&mut p, &c, w);
    }
    let regions = std::mem::take(&mut p.regions);
    Some(Content { surf, w, h, regions, split: None, live: Vec::new() })
}

/// The panel at a scroll offset, in the room there is for it: what's in view, and
/// the height of the whole of it.
#[allow(clippy::too_many_arguments)]
pub fn panel(m: &Model, icons: &mut Icons, cfg: &Config, mon: &str, view: &View, (room, scroll): (f64, f64), scale: crate::scale::Scale, pinned: bool, tucked: bool) -> Option<(Content, f64)> {
    let room = room.floor();
    let by = scroll.round();
    let (surf, mut p) = Pen::new(PANEL_W, room, cfg, &m.pal, scale, icons)?;
    p.solid = layered(cfg, m.style);
    p.cr.translate(0.0, -by);
    let c = Ctx { m, cfg, mon, pinned, tucked };
    let total = match view {
        View::Main => layers(&mut p, &c, 16.0),
        View::Menu { title, entries, .. } => menu::view(&mut p, title, entries),
        View::Palette => palette::view(&mut p, &c),
        View::Toast => notify::toast(&mut p, &c),
    };
    let h = total.min(room);
    let w = if *view == View::Toast { notify::TOAST_W } else { PANEL_W };
    if total > room {
        p.cr.translate(0.0, by);
        if total - room - by > 1.0 {
            // More below fades out, rather than looking cut off.
            let fade = cairo::LinearGradient::new(0.0, room - 36.0, 0.0, room);
            fade.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.0);
            fade.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 1.0);
            p.cr.set_operator(cairo::Operator::DestOut);
            p.cr.rectangle(0.0, room - 36.0, w, 36.0);
            if p.cr.set_source(&fade).is_ok() {
                let _ = p.cr.fill();
            }
            p.cr.set_operator(cairo::Operator::Over);
        }
        // The thumb, between the corners.
        let track = room - 64.0;
        let thumb = (track * room / total).max(24.0);
        p.fill((w - 9.0, 32.0 + (track - thumb) * (by / (total - room)).min(1.0), 3.0, thumb), 1.5, p.pal.fg, 0.3);
    }
    crate::pen::scrolled(&mut p.regions, &mut p.live, by, h);
    let regions = std::mem::take(&mut p.regions);
    Some((Content { surf, w, h, regions, split: None, live: p.live }, total))
}
