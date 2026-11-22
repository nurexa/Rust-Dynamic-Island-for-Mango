//! Top of the panel: clock, date and the island's own controls.
use super::{Ctx, PAD, clock};
use crate::pen::{Action, Pen, Region};
use pango::Weight;

pub fn panel(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let pal = p.pal;
    // The time leads, unless the clock tile has it; the day is under it.
    if c.cfg.panel.iter().any(|n| n == "clock") {
        p.text(&clock::format("%A"), 17.0, Weight::Bold, pal.fg, PAD, y + 18.0, -1.0);
        p.text(&clock::format("%B %-e, %Y"), 12.0, Weight::Medium, pal.muted, PAD, y + 39.0, -1.0);
    } else {
        p.text(&clock::format(&c.cfg.clock), 28.0, Weight::Bold, pal.fg, PAD, y + 16.0, -1.0);
        p.text(&clock::format("%A, %B %-e"), 12.0, Weight::Medium, pal.muted, PAD, y + 42.0, -1.0);
    }
    let right = p.w - PAD + 6.0;
    let top = y + 4.0;
    let rect = |i: f64| (right - 2.0 - (i + 1.0) * 34.0, top + 4.0, 32.0, 32.0);
    let cmd = &c.cfg.commands;
    let picker = c.cfg.tools.iter().any(|t| t == "wallpaper") && !c.cfg.wallpaper.dir.is_empty();
    let pickable = picker || !cmd.wallpaper_pick.is_empty();
    let count = 3.0 + f64::from(u8::from(pickable)) + f64::from(u8::from(!c.cfg.sidebar.is_empty()));
    let bar = (right - count * 34.0 - 6.0, top, count * 34.0 + 6.0, 40.0);
    p.raised(bar, 20.0);

    // On its own and in words: it's how the rest of this gets changed.
    let w = p.width("Settings", 12.5, Weight::Semibold) + 44.0;
    let r = (bar.0 - 8.0 - w, top, w, 40.0);
    p.raised(r, 20.0);
    p.icon("settings", true, 18.0, r.0 + 12.0, top + 11.0, pal.accent, 1.0);
    p.text("Settings", 12.5, Weight::Semibold, pal.fg, r.0 + 35.0, top + 20.0, -1.0);
    p.regions.push(Region::new(r, 20.0, Action::Open("settings")).tip("Change anything, no file to edit"));

    let mut next = 3.0;
    if pickable {
        let r = rect(next);
        next += 1.0;
        let pick = if picker { Action::Open("wallpaper") } else { Action::Sh(cmd.wallpaper_pick.clone()) };
        p.button("image", r, false, Region::new(r, 16.0, pick)
            .right(Action::Sh(cmd.wallpaper_next.clone()))
            .middle(Action::Sh(cmd.wallpaper_toggle.clone())).tip("Wallpaper: pick · right: random · middle: pause"));
    }
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
    y + 50.0 + 12.0
}
