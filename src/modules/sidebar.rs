//! The sidebar's sections, in the order `sidebar` gives. Where the panel already
//! has the part, this is the same module drawn narrower.
use super::{Ctx, PAD, audio, brightness, calendar, clock, media, notify};
use crate::pen::{Action, Pen, Rect, Region};
use pango::Weight;

type Section = fn(&mut Pen, &Ctx, f64) -> f64;

pub const SECTIONS: &[(&str, Section)] = &[
    ("media", media::player),
    ("calendar", calendar::panel),
    ("controls", controls),
    ("notifications", notify::sidebar),
];

/// Draws every section. Returns the height it all came to.
pub fn draw(p: &mut Pen, c: &Ctx) -> f64 {
    let mut y = header(p, c, 18.0);
    for name in &c.cfg.sidebar {
        // The player is a card already.
        let carded = name != "media";
        let top = if carded { y + super::CARD.1 } else { y };
        let end = if let Some(custom) = name.strip_prefix("custom:") {
            super::custom::panel(p, c, custom, top)
        } else if let Some((_, section)) = SECTIONS.iter().find(|(n, _)| n == name) {
            section(p, c, top)
        } else {
            top
        };
        if end <= top {
            continue;
        }
        if carded {
            let spacing = if name == "controls" { 2.0 } else { super::seat(name).1 };
            let h = end - spacing + super::CARD.1 - y;
            p.behind((PAD - super::CARD.0, y, p.w - 2.0 * (PAD - super::CARD.0), h), super::CARD_R, p.pal.card);
            y += h + super::CARD_GAP;
        } else {
            y = end + super::CARD_GAP;
        }
    }
    y + 8.0
}

fn header(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let pal = p.pal;
    let cy = y + 16.0;
    let w = p.text(&clock::format(&c.cfg.clock), 22.0, Weight::Semibold, pal.fg, PAD, cy, -1.0);
    p.text(&clock::format("%a, %b %-e"), 12.5, Weight::Medium, pal.muted, PAD + w + 10.0, cy + 3.0, -1.0);
    let r = (p.w - PAD - 26.0, y, 32.0, 32.0);
    p.raised(r, 16.0);
    p.button("close", r, false, Region::new(r, 16.0, Action::Sidebar));
    let r = (r.0 - 38.0, y, 32.0, 32.0);
    p.raised(r, 16.0);
    p.button("settings", r, false, Region::new(r, 16.0, Action::Open("settings")).tip("Settings"));
    y + 32.0 + 16.0
}

struct Tile {
    icon: &'static str,
    label: &'static str,
    on: bool,
    left: Action,
    right: Action,
}

impl Tile {
    fn draw(&self, p: &mut Pen, rect: Rect) {
        let pal = p.pal;
        let (x, y, _, h) = rect;
        let tone = if self.on {
            p.surface(rect, h / 2.0, pal.accent);
            pal.on_accent
        } else {
            p.raised(rect, 16.0);
            pal.fg
        };
        p.icon(self.icon, self.on, 20.0, x + 16.0, y + h / 2.0 - 10.0, tone, 1.0);
        p.text(self.label, 12.5, Weight::Semibold, tone, x + 44.0, y + h / 2.0, rect.2 - 52.0);
        let radius = if self.on { h / 2.0 } else { 16.0 };
        p.regions.push(Region::new(rect, radius, self.left.clone()).right(self.right.clone()));
    }
}

fn tiles(c: &Ctx) -> Vec<Tile> {
    let (m, cfg) = (c.m, c.cfg);
    let has = |list: &[String], name: &str| list.iter().any(|n| n == name);
    let off = |name: &str| cfg.disable.iter().any(|d| d == name);
    let mut tiles = Vec::new();
    if let Some(on) = m.radio.filter(|_| !off("wifi")) {
        tiles.push(Tile {
            icon: if on { "wifi" } else { "wifi_off" },
            label: "Wi-Fi",
            on,
            left: Action::Sh(format!("nmcli radio wifi {}", if on { "off" } else { "on" })),
            right: if has(&cfg.tools, "wifi") { Action::Open("wifi") } else { Action::Sh(cfg.commands.wifi.clone()) },
        });
    }
    if let Some(bt) = m.bluetooth.as_ref().filter(|_| !off("bluetooth")) {
        tiles.push(Tile {
            icon: if bt.powered { "bluetooth" } else { "bluetooth_disabled" },
            label: "Bluetooth",
            on: bt.powered,
            left: Action::Bluetooth,
            right: if has(&cfg.tools, "bluetooth") { Action::Open("bluetooth") } else { Action::None },
        });
    }
    if m.night.usable() && !off("nightlight") {
        tiles.push(Tile { icon: "bedtime", label: "Night light", on: m.night.on, left: Action::Night, right: Action::None });
    }
    if cfg.notifications {
        let icon = if m.notes.dnd { "do_not_disturb_on" } else { "notifications" };
        tiles.push(Tile { icon, label: "Do not disturb", on: m.notes.dnd, left: Action::Dnd, right: Action::None });
    }
    tiles
}

fn controls(p: &mut Pen, c: &Ctx, mut y: f64) -> f64 {
    let (gap, h) = (8.0, 48.0);
    let full = p.w - 2.0 * PAD;
    let half = (full - gap) / 2.0;
    let tiles = tiles(c);
    for (i, tile) in tiles.iter().enumerate() {
        // An odd one out takes the whole row.
        let w = if i + 1 == tiles.len() && i % 2 == 0 { full } else { half };
        tile.draw(p, (PAD + (i % 2) as f64 * (half + gap), y + (i / 2) as f64 * (h + gap), w, h));
    }
    if !tiles.is_empty() {
        y += tiles.len().div_ceil(2) as f64 * (h + gap) + 6.0;
    }
    let off = |name: &str| c.cfg.disable.iter().any(|d| d == name);
    if !off("audio") {
        y = audio::panel(p, c, y);
    }
    if !off("brightness") {
        y = brightness::panel(p, c, y);
    }
    y
}
