//! Top of the panel: clock, date and the island's own controls.
use super::{Ctx, PAD, clock};
use crate::pen::{Action, Pen, Region};
use pango::Weight;

pub fn panel(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let pal = p.pal;
    let cy = y + 16.0;
    let w = p.text(&clock::format(&c.cfg.clock), 26.0, Weight::Semibold, pal.fg, PAD, cy, -1.0);
    p.text(&clock::format("%A, %B %-e"), 12.5, Weight::Medium, pal.muted, PAD + w + 12.0, cy + 4.0, -1.0);
    let right = p.w - PAD;
    let rect = |i: f64| (right + 6.0 - (i + 1.0) * 34.0, y, 32.0, 32.0);
    let cmd = &c.cfg.commands;
    let mut next = 3.0;
    let picker = c.cfg.tools.iter().any(|t| t == "wallpaper") && !c.cfg.wallpaper.dir.is_empty();
    if picker || !cmd.wallpaper_pick.is_empty() {
        let r = rect(next);
        next += 1.0;
        let pick = if picker { Action::Open("wallpaper") } else { Action::Sh(cmd.wallpaper_pick.clone()) };
        p.button("image", r, false, Region::new(r, 16.0, pick)
            .right(Action::Sh(cmd.wallpaper_next.clone()))
            .middle(Action::Sh(cmd.wallpaper_toggle.clone())).tip("Wallpaper: pick · right: random · middle: pause"));
    }
    let r = rect(next);
    next += 1.0;
    p.button("settings", r, false, Region::new(r, 16.0, Action::Open("settings")).tip("Settings"));
    if !c.cfg.sidebar.is_empty() {
        let r = rect(next);
        p.button("dock_to_right", r, c.m.sidebar_open, Region::new(r, 16.0, Action::Sidebar).tip("Sidebar"));
    }
    let r = rect(2.0);
    p.button("home", r, false, Region::new(r, 16.0, Action::Reset).tip("Back to top centre"));
    let r = rect(1.0);
    p.button(if c.tucked { "keyboard_arrow_down" } else { "keyboard_arrow_up" }, r, c.tucked, Region::new(r, 16.0, Action::Tuck).tip("Tuck away"));
    let r = rect(0.0);
    p.button("keep", r, c.pinned, Region::new(r, 16.0, Action::Pin).tip("Keep open"));
    y + 32.0 + 14.0
}
