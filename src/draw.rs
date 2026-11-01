//! Content layout. Both states render once into a cached surface, with their
//! click targets, and the frame loop just composites those. Nothing here runs
//! per animation frame.
use crate::{
    Model, bus::MenuEntry, icons::Icons, mango, net,
    settings::Config,
    theme::{Palette, Rgb, mix},
};
use cairo::{Context, Format, ImageSurface};
use pango::{AttrFontFeatures, AttrList, EllipsizeMode, FontDescription, Weight};

pub const PILL_H: f64 = 32.0;
pub const PANEL_W: f64 = 560.0;
pub const MAX_PANEL_H: f64 = 680.0;
pub const MAX_COMPACT_W: f64 = 680.0;
const MIN_COMPACT_W: f64 = 168.0;
const PAD: f64 = 20.0;
const PERSISTENT_TAGS: u32 = 5; // same as the old bar: always shown so chips don't jump

pub type Rect = (f64, f64, f64, f64);

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    None,
    View { tag: u32, mon: String },
    MoveTo(u32),
    Focus(u64),
    Sh(&'static str),
    Mute,
    Volume,
    Pin,
    Reset,
    Tuck,
    TrayClick(usize),
    TrayRight(usize),
    TrayMiddle(usize),
    MenuClick(i32),
    MenuSub(i32),
    MenuBack,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scroll {
    Tags,
    Volume,
}

#[derive(Clone, Debug)]
pub struct Region {
    pub rect: Rect,
    pub radius: f64,
    pub left: Action,
    pub right: Action,
    pub middle: Action,
    pub scroll: Option<Scroll>,
    pub tip: Option<String>,
}

impl Region {
    fn new(rect: Rect, radius: f64, left: Action) -> Self {
        Region { rect, radius, left, right: Action::None, middle: Action::None, scroll: None, tip: None }
    }
    fn right(mut self, a: Action) -> Self {
        self.right = a;
        self
    }
    fn middle(mut self, a: Action) -> Self {
        self.middle = a;
        self
    }
    fn tip(mut self, t: impl Into<String>) -> Self {
        self.tip = Some(t.into());
        self
    }
}

pub struct Content {
    pub surf: ImageSurface,
    pub w: f64,
    pub h: f64,
    pub regions: Vec<Region>,
}

/// What a panel is showing: the system view, or a tray app's menu.
#[derive(Clone, Debug, PartialEq)]
pub enum View {
    Main,
    Menu { item: usize, title: String, entries: Vec<MenuEntry>, root: Vec<MenuEntry> },
}

pub fn clock(fmt: &str) -> String {
    let s = pango::glib::DateTime::now_local().and_then(|d| d.format(fmt)).map(|s| s.to_string());
    s.unwrap_or_default().trim_start_matches('0').to_owned()
}

fn rate(v: f64) -> String {
    match v {
        v if v >= 1e9 => format!("{:.1} GB/s", v / 1e9),
        v if v >= 1e6 => format!("{:.1} MB/s", v / 1e6),
        v if v >= 1e3 => format!("{:.0} KB/s", v / 1e3),
        v => format!("{v:.0} B/s"),
    }
}

fn gib(v: f64) -> String {
    format!("{:.1}", v / (1024.0 * 1024.0 * 1024.0))
}

struct Pen<'a> {
    cr: Context,
    layout: pango::Layout,
    font: &'a str,
    pal: &'a Palette,
    scale: i32,
    icons: &'a mut Icons,
    regions: Vec<Region>,
}

impl<'a> Pen<'a> {
    fn new(w: f64, h: f64, font: &'a str, pal: &'a Palette, scale: i32, icons: &'a mut Icons) -> Option<(ImageSurface, Self)> {
        let s = scale as f64;
        let surf = ImageSurface::create(Format::ARgb32, (w * s) as i32, (h * s) as i32).ok()?;
        surf.set_device_scale(s, s);
        let cr = Context::new(&surf).ok()?;
        let layout = pangocairo::functions::create_layout(&cr);
        Some((surf, Pen { cr, layout, font, pal, scale, icons, regions: Vec::new() }))
    }

    fn set(&self, (r, g, b): Rgb, a: f64) {
        self.cr.set_source_rgba(r, g, b, a);
    }

    fn prepare(&self, s: &str, size: f64, weight: Weight, max_w: f64) {
        let mut d = FontDescription::new();
        d.set_family(self.font);
        d.set_weight(weight);
        d.set_absolute_size(size * pango::SCALE as f64);
        self.layout.set_font_description(Some(&d));
        // Tabular figures keep clocks and percentages from shimmering as they tick.
        let attrs = AttrList::new();
        attrs.insert(AttrFontFeatures::new("tnum"));
        self.layout.set_attributes(Some(&attrs));
        self.layout.set_width((max_w * pango::SCALE as f64) as i32);
        self.layout.set_ellipsize(if max_w > 0.0 { EllipsizeMode::End } else { EllipsizeMode::None });
        self.layout.set_text(s);
    }

    fn width(&self, s: &str, size: f64, weight: Weight) -> f64 {
        self.prepare(s, size, weight, -1.0);
        self.layout.pixel_size().0 as f64
    }

    /// Left-aligned at x, vertically centred on cy. Returns the width drawn.
    fn text(&self, s: &str, size: f64, weight: Weight, c: Rgb, x: f64, cy: f64, max_w: f64) -> f64 {
        self.prepare(s, size, weight, max_w);
        let (w, h) = self.layout.pixel_size();
        self.set(c, 1.0);
        self.cr.move_to(x, cy - h as f64 / 2.0);
        pangocairo::functions::show_layout(&self.cr, &self.layout);
        w as f64
    }

    fn right(&self, s: &str, size: f64, weight: Weight, c: Rgb, x_end: f64, cy: f64) -> f64 {
        let w = self.width(s, size, weight);
        self.text(s, size, weight, c, x_end - w, cy, -1.0);
        w
    }

    fn pill(&self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        let cr = &self.cr;
        let pi = std::f64::consts::PI;
        let r = r.min(w / 2.0).min(h / 2.0);
        cr.new_sub_path();
        cr.arc(x + w - r, y + r, r, -pi / 2.0, 0.0);
        cr.arc(x + w - r, y + h - r, r, 0.0, pi / 2.0);
        cr.arc(x + r, y + h - r, r, pi / 2.0, pi);
        cr.arc(x + r, y + r, r, pi, 1.5 * pi);
        cr.close_path();
    }

    fn fill(&self, rect: Rect, r: f64, c: Rgb, a: f64) {
        self.pill(rect.0, rect.1, rect.2, rect.3, r);
        self.set(c, a);
        let _ = self.cr.fill();
    }

    /// `size` is the logical edge: rasters can outlive a scale change, so their
    /// density comes from the raster itself rather than from this output.
    fn image(&self, img: &ImageSurface, x: f64, y: f64, size: f64, a: f64) {
        let s = img.width() as f64 / size;
        img.set_device_scale(s, s);
        if self.cr.set_source_surface(img, x, y).is_ok() {
            let _ = self.cr.paint_with_alpha(a);
        }
    }

    fn symbol(&mut self, name: &'static str, size: f64, x: f64, y: f64, c: Rgb) {
        let Some(mask) = self.icons.symbol(name, (size * self.scale as f64) as i32) else { return };
        mask.set_device_scale(self.scale as f64, self.scale as f64);
        self.set(c, 1.0);
        let _ = self.cr.mask_surface(&mask, x, y);
    }

    /// The app's real icon, or a lettered tile when nothing on disk matches.
    fn app(&mut self, appid: &str, size: f64, x: f64, y: f64, alpha: f64) {
        if let Some(img) = self.icons.app(appid, (size * self.scale as f64) as i32) {
            return self.image(&img, x, y, size, alpha);
        }
        let tile = mix(self.pal.accent, self.pal.bg, 0.68);
        self.fill((x, y, size, size), size * 0.28, tile, alpha);
        let letter: String = appid.chars().next().unwrap_or('?').to_uppercase().collect();
        let w = self.width(&letter, size * 0.56, Weight::Semibold);
        self.text(&letter, size * 0.56, Weight::Semibold, self.pal.fg, x + (size - w) / 2.0, y + size / 2.0, -1.0);
    }

    fn button(&mut self, symbol: &'static str, rect: Rect, on: bool, region: Region) {
        if on {
            self.fill(rect, rect.2 / 2.0, self.pal.accent, 0.22);
        }
        let size = 16.0;
        let c = if on { self.pal.accent } else { self.pal.fg };
        self.symbol(symbol, size, rect.0 + (rect.2 - size) / 2.0, rect.1 + (rect.3 - size) / 2.0, c);
        self.regions.push(region);
    }
}

fn volume_symbol(volume: u32, muted: bool) -> &'static str {
    match volume {
        _ if muted => "audio-volume-muted",
        0..=33 => "audio-volume-low",
        34..=66 => "audio-volume-medium",
        _ => "audio-volume-high",
    }
}

fn net_symbol(state: &net::State) -> &'static str {
    match state {
        net::State::Wifi { strength, .. } => match strength {
            0..=19 => "network-wireless-signal-none",
            20..=39 => "network-wireless-signal-weak",
            40..=59 => "network-wireless-signal-ok",
            60..=79 => "network-wireless-signal-good",
            _ => "network-wireless-signal-excellent",
        },
        net::State::Wired { .. } => "network-wired",
        net::State::Other { .. } => "network-transmit-receive",
        net::State::Offline | net::State::Unknown => "network-offline",
    }
}

fn tags_with_windows<'a>(m: &'a Model, mon: &'a mango::Monitor) -> impl Iterator<Item = (&'a mango::Tag, Vec<&'a mango::Client>)> {
    mon.tags.iter().map(|t| (t, mango::clients_on(&m.clients, &mon.name, t.index).collect()))
}

/// The pill: occupied workspaces with their apps, then the clock.
pub fn compact(m: &Model, icons: &mut Icons, cfg: &Config, mon: &str, scale: i32, flash: bool) -> Option<Content> {
    let (surf, mut p) = Pen::new(MAX_COMPACT_W, PILL_H, &cfg.font, &m.pal, scale, icons)?;
    let pal = &m.pal;
    let cy = PILL_H / 2.0;

    if flash {
        if let Some(a) = &m.audio {
            p.symbol(volume_symbol(a.volume, a.muted), 16.0, 16.0, cy - 8.0, pal.fg);
            let bar = (42.0, cy - 2.0, 104.0, 4.0);
            p.fill(bar, 2.0, pal.fg, 0.18);
            let on = if a.muted { pal.muted } else { pal.accent };
            p.fill((bar.0, bar.1, bar.2 * (a.volume.min(100) as f64 / 100.0), bar.3), 2.0, on, 1.0);
            p.right(&format!("{}", a.volume), 13.0, Weight::Semibold, pal.fg, 184.0, cy);
            return Some(Content { surf, w: 200.0, h: PILL_H, regions: Vec::new() });
        }
    }

    let mut x = 10.0;
    if let Some(t) = m.media.as_ref().filter(|t| t.playing) {
        let tip = if t.artist.is_empty() { t.title.clone() } else { format!("{} – {}", t.title, t.artist) };
        match &m.art_small {
            Some(a) => p.image(a, x, cy - 10.0, 20.0, 1.0),
            None => p.symbol("audio-x-generic", 16.0, x + 2.0, cy - 8.0, pal.accent),
        }
        p.regions.push(Region::new((x - 2.0, cy - 12.0, 24.0, 24.0), 6.0, Action::Sh("playerctl play-pause"))
            .right(Action::Sh("playerctl next")).middle(Action::Sh("playerctl previous")).tip(tip));
        x += 26.0;
    }

    if let Some(monitor) = m.monitors.iter().find(|x| x.name == mon) {
        let total: usize = m.clients.iter().filter(|c| c.monitor == mon && !c.is_minimized).count();
        let max_icons = if total > 12 { 2 } else if total > 7 { 3 } else { 5 };
        for (tag, windows) in tags_with_windows(m, monitor) {
            if windows.is_empty() && !tag.is_active {
                continue;
            }
            let label = (tag.index % 10).to_string();
            let digit = p.width(&label, 12.5, Weight::Semibold);
            let shown = windows.len().min(max_icons);
            let extra = windows.len() - shown;
            let more = (extra > 0).then(|| format!("+{extra}"));
            let more_w = more.as_ref().map_or(0.0, |s| p.width(s, 11.0, Weight::Medium) + 4.0);
            let w = 8.0 + digit + 8.0 + shown as f64 * 22.0 + more_w + if shown > 0 || more.is_some() { 4.0 } else { 0.0 };
            if x + w + 90.0 > MAX_COMPACT_W {
                break;
            }
            let rect = (x, cy - 12.5, w, 25.0);
            let color = if tag.is_urgent { pal.error } else if tag.is_active { pal.accent } else { pal.muted };
            if tag.is_active {
                p.fill(rect, 12.5, pal.accent, 0.16);
            }
            p.text(&label, 12.5, Weight::Semibold, color, x + 8.0, cy, -1.0);
            let icon_alpha = if tag.is_active { 1.0 } else { 0.66 };
            let mut ix = x + 8.0 + digit + 6.0;
            p.regions.push(Region::new(rect, 12.5, Action::View { tag: tag.index, mon: mon.into() })
                .right(Action::MoveTo(tag.index)));
            for c in windows.iter().take(shown) {
                let id = c.appid.as_deref().unwrap_or("");
                p.app(id, 18.0, ix, cy - 9.0, icon_alpha);
                if c.is_urgent {
                    p.fill((ix + 14.0, cy - 10.0, 6.0, 6.0), 3.0, pal.error, 1.0);
                } else if c.is_focused {
                    p.fill((ix + 7.5, cy + 10.0, 3.0, 3.0), 1.5, pal.fg, 0.9);
                }
                p.regions.push(Region::new((ix - 2.0, cy - 12.0, 22.0, 24.0), 7.0, Action::Focus(c.id))
                    .right(Action::MoveTo(tag.index))
                    .tip(c.title.clone().or_else(|| c.appid.clone()).unwrap_or_default()));
                ix += 22.0;
            }
            if let Some(s) = more {
                p.text(&s, 11.0, Weight::Medium, pal.muted, ix + 2.0, cy, -1.0);
            }
            x += w + 3.0;
        }
    }

    let time = clock(&cfg.clock);
    let time_w = p.width(&time, 13.0, Weight::Semibold);
    let offline = m.net == net::State::Offline;
    let w = (x + 9.0 + if offline { 22.0 } else { 0.0 } + time_w + 16.0).max(MIN_COMPACT_W);
    if offline {
        p.symbol("network-offline", 14.0, w - 16.0 - time_w - 20.0, cy - 7.0, pal.muted);
    }
    p.text(&time, 13.0, Weight::Semibold, pal.fg, w - 16.0 - time_w, cy, -1.0);
    let regions = std::mem::take(&mut p.regions);
    Some(Content { surf, w, h: PILL_H, regions })
}

fn chips(p: &mut Pen, m: &Model, mon: &mango::Monitor, mut y: f64) -> f64 {
    let pal = p.pal;
    let (left, right) = (PAD, PANEL_W - PAD);
    let h = 36.0;
    let mut x = left;
    for (tag, windows) in tags_with_windows(m, mon) {
        if windows.is_empty() && !tag.is_active && tag.index > PERSISTENT_TAGS {
            continue;
        }
        let label = (tag.index % 10).to_string();
        let digit = p.width(&label, 13.0, Weight::Semibold);
        let shown = windows.len().min(6);
        let w = if windows.is_empty() { 36.0 } else { 12.0 + digit + 8.0 + shown as f64 * 26.0 + 8.0 };
        if x + w > right && x > left {
            x = left;
            y += h + 6.0;
        }
        let rect = (x, y, w, h);
        let lead = tag.is_active && mon.active;
        let color = match () {
            _ if tag.is_urgent => pal.error,
            _ if lead => pal.on_accent,
            _ if tag.is_active => pal.fg,
            _ if windows.is_empty() => pal.muted,
            _ => pal.fg,
        };
        if lead {
            p.fill(rect, 13.0, pal.accent, 1.0);
        } else if tag.is_active {
            p.fill(rect, 13.0, pal.accent, 0.22);
        } else if !windows.is_empty() {
            p.fill(rect, 13.0, pal.fg, 0.07);
        }
        let digit_x = if windows.is_empty() { x + (w - digit) / 2.0 } else { x + 12.0 };
        p.text(&label, 13.0, Weight::Semibold, color, digit_x, y + h / 2.0, -1.0);
        p.regions.push(Region::new(rect, 13.0, Action::View { tag: tag.index, mon: mon.name.clone() })
            .right(Action::MoveTo(tag.index)).tip(format!("Workspace {label}")));
        let mut ix = x + 12.0 + digit + 8.0;
        for c in windows.iter().take(shown) {
            let id = c.appid.as_deref().unwrap_or("");
            if lead {
                // Colored app icons need a calm plate to sit on over the accent.
                p.fill((ix - 1.5, y + 6.5, 23.0, 23.0), 7.0, pal.bg, 0.72);
            }
            p.app(id, 20.0, ix, y + 8.0, 1.0);
            if c.is_focused {
                let dot = if lead { pal.on_accent } else { pal.fg };
                p.fill((ix + 8.5, y + h - 5.0, 3.0, 3.0), 1.5, dot, 1.0);
            }
            p.regions.push(Region::new((ix - 3.0, y + 3.0, 26.0, h - 6.0), 8.0, Action::Focus(c.id))
                .right(Action::MoveTo(tag.index))
                .tip(c.title.clone().or_else(|| c.appid.clone()).unwrap_or_default()));
            ix += 26.0;
        }
        x += w + 6.0;
    }
    y + h
}

fn tile(p: &mut Pen, rect: Rect, label: &str, value: &str, sub: &str, bar: Option<f64>) {
    let pal = p.pal;
    let (x, y, w, h) = rect;
    p.fill(rect, 14.0, pal.fg, 0.055);
    p.text(label, 10.5, Weight::Medium, pal.muted, x + 12.0, y + 15.0, -1.0);
    if !sub.is_empty() {
        p.right(sub, 11.0, Weight::Medium, pal.muted, x + w - 12.0, y + 15.0);
    }
    p.text(value, 17.0, Weight::Semibold, pal.fg, x + 12.0, y + 35.0, w - 24.0);
    let track = (x + 12.0, y + h - 10.0, w - 24.0, 3.0);
    if let Some(frac) = bar {
        p.fill(track, 1.5, pal.fg, 0.1);
        p.fill((track.0, track.1, track.2 * frac.clamp(0.015, 1.0), track.3), 1.5, pal.accent, 1.0);
    }
}

fn stats(p: &mut Pen, m: &Model, y: f64) -> f64 {
    let s = &m.sys;
    let temp = |t: Option<f64>| t.map_or(String::new(), |t| format!("{t:.0}°"));
    let mut tiles: Vec<(&str, String, String, Option<f64>)> = Vec::new();
    if let Some(c) = s.cpu {
        tiles.push(("CPU", format!("{c:.0}%"), temp(s.cpu_temp), Some(c / 100.0)));
    }
    if let Some(g) = s.gpu {
        tiles.push(("GPU", format!("{g:.0}%"), temp(s.gpu_temp), Some(g / 100.0)));
    }
    if let Some((used, total)) = s.mem {
        tiles.push(("RAM", format!("{} GB", gib(used)), format!("of {}", gib(total)), Some(used / total)));
    }
    tiles.push(("NET", format!("↓ {}", rate(s.rx)), format!("↑ {}", rate(s.tx)), None));
    let n = tiles.len() as f64;
    let gap = 8.0;
    let w = (PANEL_W - 2.0 * PAD - (n - 1.0) * gap) / n;
    let h = 58.0;
    for (i, (label, value, sub, bar)) in tiles.iter().enumerate() {
        tile(p, (PAD + i as f64 * (w + gap), y, w, h), label, value, sub, *bar);
    }
    y + h
}

fn media(p: &mut Pen, m: &Model, y: f64) -> f64 {
    let Some(t) = &m.media else { return y };
    let pal = p.pal;
    let size = 58.0;
    match &m.art_large {
        Some(a) => {
            p.pill(PAD, y, size, size, 12.0);
            let _ = p.cr.save();
            let _ = p.cr.clip();
            p.image(a, PAD, y, size, 1.0);
            let _ = p.cr.restore();
        }
        None => {
            p.fill((PAD, y, size, size), 12.0, pal.fg, 0.07);
            p.symbol("audio-x-generic", 24.0, PAD + 17.0, y + 17.0, pal.muted);
        }
    }
    let x = PAD + size + 14.0;
    let max = PANEL_W - PAD - x - 128.0;
    p.text(&t.title, 14.0, Weight::Semibold, pal.fg, x, y + 13.0, max);
    p.text(&t.artist, 12.0, Weight::Normal, pal.muted, x, y + 31.0, max);
    if t.length > 0.0 {
        let bar = (x, y + 47.0, max, 3.0);
        p.fill(bar, 1.5, pal.fg, 0.14);
        p.fill((bar.0, bar.1, bar.2 * (t.now() / t.length).clamp(0.0, 1.0), bar.3), 1.5, pal.accent, 1.0);
    }
    let cy = y + size / 2.0;
    let end = PANEL_W - PAD;
    let play = if t.playing { "media-playback-pause" } else { "media-playback-start" };
    let cmd = |s| Action::Sh(s);
    let r = (end - 118.0, cy - 17.0, 34.0, 34.0);
    p.button("media-skip-backward", r, false, Region::new(r, 17.0, cmd("playerctl previous")));
    let r = (end - 80.0, cy - 21.0, 42.0, 42.0);
    p.fill(r, 21.0, pal.fg, 0.1);
    p.button(play, r, false, Region::new(r, 21.0, cmd("playerctl play-pause")));
    let r = (end - 34.0, cy - 17.0, 34.0, 34.0);
    p.button("media-skip-forward", r, false, Region::new(r, 17.0, cmd("playerctl next")));
    y + size
}

fn audio(p: &mut Pen, m: &Model, y: f64) -> f64 {
    let Some(a) = &m.audio else { return y };
    let pal = p.pal;
    let h = 30.0;
    let cy = y + h / 2.0;
    let mute = (PAD - 6.0, y, 30.0, h);
    let symbol = volume_symbol(a.volume, a.muted);
    p.button(symbol, mute, a.muted, Region::new(mute, 15.0, Action::Mute).right(Action::Sh("pavucontrol")).tip("Mute"));
    let label_w = 168.0;
    p.text(&a.device, 12.5, Weight::Medium, pal.fg, PAD + 30.0, cy, label_w);
    let x = PAD + 30.0 + label_w + 12.0;
    let w = PANEL_W - PAD - 40.0 - x;
    p.fill((x, cy - 2.0, w, 4.0), 2.0, pal.fg, 0.14);
    let frac = a.volume.min(100) as f64 / 100.0;
    let on = if a.muted { pal.muted } else { pal.accent };
    p.fill((x, cy - 2.0, w * frac, 4.0), 2.0, on, 1.0);
    p.fill((x + w * frac - 6.0, cy - 6.0, 12.0, 12.0), 6.0, pal.fg, 1.0);
    p.right(&format!("{}%", a.volume), 12.0, Weight::Medium, pal.muted, PANEL_W - PAD, cy);
    let mut slider = Region::new((x - 6.0, y, w + 12.0, h), 8.0, Action::Volume).right(Action::Sh("pavucontrol"));
    slider.scroll = Some(Scroll::Volume);
    p.regions.push(slider);
    y + h
}

fn status(p: &mut Pen, m: &Model, y: f64) -> f64 {
    let pal = p.pal;
    let h = 30.0;
    let cy = y + h / 2.0;
    let label = match &m.net {
        net::State::Wifi { ssid, .. } => ssid.clone(),
        net::State::Wired { name } | net::State::Other { name } => name.clone(),
        net::State::Offline => "Disconnected".into(),
        net::State::Unknown => "Network".into(),
    };
    p.symbol(net_symbol(&m.net), 16.0, PAD + 1.0, cy - 8.0, pal.fg);
    let w = p.text(&label, 12.5, Weight::Medium, pal.fg, PAD + 30.0, cy, 168.0);
    p.regions.push(Region::new((PAD - 6.0, y, 40.0 + w, h), 10.0, Action::Sh("~/.config/mango/scripts/wifi-menu"))
        .right(Action::Sh("kitty -e nmtui")));
    if let Some((pct, charging)) = m.sys.battery {
        let x = PAD + 30.0 + w + 16.0;
        let symbol = match (pct as u32 / 10 * 10, charging) {
            (0..=19, false) => "battery-level-10",
            (0..=39, false) => "battery-level-30",
            (0..=69, false) => "battery-level-50",
            (0..=89, false) => "battery-level-80",
            (_, false) => "battery-level-100",
            (_, true) => "battery-level-50-charging",
        };
        p.symbol(symbol, 16.0, x, cy - 8.0, pal.fg);
        p.text(&format!("{pct:.0}%"), 12.5, Weight::Medium, pal.fg, x + 22.0, cy, -1.0);
    }
    let mut x = PANEL_W - PAD - 30.0;
    for (i, (item, icon)) in m.tray.iter().enumerate().rev() {
        let r = (x, y, 30.0, h);
        match icon {
            Some((img, false)) => p.image(img, x + 6.0, cy - 9.0, 18.0, 1.0),
            Some((mask, true)) => {
                let s = mask.width() as f64 / 18.0;
                mask.set_device_scale(s, s);
                p.set(pal.fg, 1.0);
                let _ = p.cr.mask_surface(mask, x + 6.0, cy - 9.0);
            }
            None => p.app(&item.title, 18.0, x + 6.0, cy - 9.0, 1.0),
        }
        p.regions.push(Region::new(r, 10.0, Action::TrayClick(i))
            .right(Action::TrayRight(i)).middle(Action::TrayMiddle(i)).tip(item.title.clone()));
        x -= 32.0;
    }
    y + h
}

fn main_view(p: &mut Pen, m: &Model, cfg: &Config, pinned: bool, tucked: bool) -> f64 {
    let pal = p.pal;
    let mut y = 16.0;
    let cy = y + 16.0;
    let w = p.text(&clock(&cfg.clock), 26.0, Weight::Semibold, pal.fg, PAD, cy, -1.0);
    p.text(&clock("%A, %B %-e"), 12.5, Weight::Medium, pal.muted, PAD + w + 12.0, cy + 4.0, -1.0);
    let top = cy - 16.0;
    let rect = |i: f64| (PANEL_W - PAD + 6.0 - (i + 1.0) * 34.0, top, 32.0, 32.0);
    let r = rect(3.0);
    p.button("preferences-desktop-wallpaper", r, false, Region::new(r, 16.0, Action::Sh("~/.config/mango/scripts/wall pick"))
        .right(Action::Sh("~/.config/mango/scripts/wall next"))
        .middle(Action::Sh("~/.config/mango/scripts/wall toggle")).tip("Wallpaper: pick · right: random · middle: pause"));
    let r = rect(2.0);
    p.button("go-home", r, false, Region::new(r, 16.0, Action::Reset).tip("Back to top centre"));
    let r = rect(1.0);
    p.button("pan-up", r, tucked, Region::new(r, 16.0, Action::Tuck).tip("Tuck away"));
    let r = rect(0.0);
    p.button("view-pin", r, pinned, Region::new(r, 16.0, Action::Pin).tip("Keep open"));
    y += 32.0 + 14.0;

    for mon in &m.monitors {
        let cy = y + 7.0;
        let mut x = PAD;
        if mon.active {
            p.fill((x + 1.0, cy - 3.0, 6.0, 6.0), 3.0, pal.accent, 1.0);
            x += 13.0;
        }
        let w = p.text(&mon.name, 11.0, Weight::Semibold, pal.muted, x, cy, -1.0);
        p.text(&mon.layout_symbol, 11.0, Weight::Medium, pal.muted, x + w + 8.0, cy, -1.0);
        y = chips(p, m, mon, y + 20.0) + 14.0;
    }
    if m.media.is_some() {
        y = media(p, m, y) + 16.0;
    }
    y = stats(p, m, y) + 12.0;
    if m.audio.is_some() {
        y = audio(p, m, y) + 2.0;
    }
    status(p, m, y) + 12.0
}

fn menu_view(p: &mut Pen, title: &str, entries: &[MenuEntry], depth_back: bool) -> f64 {
    let pal = p.pal;
    let mut y = 14.0;
    let r = (PAD - 6.0, y, 32.0, 32.0);
    p.button("go-previous", r, false, Region::new(r, 16.0, Action::MenuBack));
    let _ = depth_back;
    p.text(title, 14.0, Weight::Semibold, pal.fg, PAD + 34.0, y + 16.0, PANEL_W - 2.0 * PAD - 40.0);
    y += 40.0;
    for e in entries {
        if y + 44.0 > MAX_PANEL_H {
            break;
        }
        if e.separator {
            p.fill((PAD, y + 4.0, PANEL_W - 2.0 * PAD, 1.0), 0.5, pal.fg, 0.1);
            y += 9.0;
            continue;
        }
        let rect = (PAD - 6.0, y, PANEL_W - 2.0 * PAD + 12.0, 32.0);
        let cy = y + 16.0;
        let c = if e.enabled { pal.fg } else { pal.muted };
        if e.checked == Some(true) {
            p.symbol("object-select", 14.0, PAD, cy - 7.0, pal.accent);
        }
        let inset = if e.checked.is_some() { 24.0 } else { 4.0 };
        p.text(&e.label, 13.0, Weight::Normal, c, PAD + inset, cy, PANEL_W - 2.0 * PAD - 40.0);
        if !e.children.is_empty() {
            p.symbol("pan-end", 14.0, PANEL_W - PAD - 14.0, cy - 7.0, pal.muted);
        }
        if e.enabled {
            let action = if e.children.is_empty() { Action::MenuClick(e.id) } else { Action::MenuSub(e.id) };
            p.regions.push(Region::new(rect, 10.0, action));
        }
        y += 32.0;
    }
    y + 10.0
}

pub fn panel(m: &Model, icons: &mut Icons, cfg: &Config, view: &View, scale: i32, pinned: bool, tucked: bool) -> Option<Content> {
    let (surf, mut p) = Pen::new(PANEL_W, MAX_PANEL_H, &cfg.font, &m.pal, scale, icons)?;
    let h = match view {
        View::Main => main_view(&mut p, m, cfg, pinned, tucked),
        View::Menu { title, entries, root, .. } => menu_view(&mut p, title, entries, entries != root),
    };
    let regions = std::mem::take(&mut p.regions);
    Some(Content { surf, w: PANEL_W, h: h.min(MAX_PANEL_H), regions })
}
