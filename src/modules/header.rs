//! Top of the panel: clock, date and the island's own controls.
use super::{Ctx, PAD, PANEL_W, clock};
use crate::pen::{Action, Pen, Region};
use pango::Weight;

pub fn panel(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let pal = p.pal;
    let cy = y + 16.0;
    let w = p.text(&clock::format(&c.cfg.clock), 26.0, Weight::Semibold, pal.fg, PAD, cy, -1.0);
    p.text(&clock::format("%A, %B %-e"), 12.5, Weight::Medium, pal.muted, PAD + w + 12.0, cy + 4.0, -1.0);
    let rect = |i: f64| (PANEL_W - PAD + 6.0 - (i + 1.0) * 34.0, y, 32.0, 32.0);
    let cmd = &c.cfg.commands;
    if !cmd.wallpaper_pick.is_empty() {
        let r = rect(3.0);
        p.button("preferences-desktop-wallpaper", r, false, Region::new(r, 16.0, Action::Sh(cmd.wallpaper_pick.clone()))
            .right(Action::Sh(cmd.wallpaper_next.clone()))
            .middle(Action::Sh(cmd.wallpaper_toggle.clone())).tip("Wallpaper: pick · right: random · middle: pause"));
    }
    let r = rect(2.0);
    p.button("go-home", r, false, Region::new(r, 16.0, Action::Reset).tip("Back to top centre"));
    let r = rect(1.0);
    p.button("pan-up", r, c.tucked, Region::new(r, 16.0, Action::Tuck).tip("Tuck away"));
    let r = rect(0.0);
    p.button("view-pin", r, c.pinned, Region::new(r, 16.0, Action::Pin).tip("Keep open"));
    y + 32.0 + 14.0
}
