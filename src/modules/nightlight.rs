//! Night light: click the icon or label to toggle, drag the slider for warmth
//! (right is warmer). Hidden when the compositor has no gamma control.
use super::{Ctx, PAD, PANEL_W};
use crate::nightlight::{MAX_K, MIN_K};
use crate::pen::{Action, Pen, Region};
use pango::Weight;

pub fn panel(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let n = &c.m.night;
    if !n.usable() {
        return y;
    }
    let pal = p.pal;
    let h = 30.0;
    let cy = y + h / 2.0;
    let label_w = 168.0;
    let tw = p.text("Night light", 12.5, Weight::Medium, pal.fg, PAD + 30.0, cy, label_w);
    // The icon and its label are one toggle.
    let icon = (PAD - 6.0, y, 30.0, h);
    p.button("night-light", icon, n.on, Region::new((icon.0, y, icon.2 + tw + 10.0, h), 15.0, Action::Night));
    let x = PAD + 30.0 + label_w + 12.0;
    let w = PANEL_W - PAD - 40.0 - x;
    p.fill((x, cy - 2.0, w, 4.0), 2.0, pal.fg, 0.14);
    let frac = (MAX_K - n.warmth) as f64 / (MAX_K - MIN_K) as f64;
    let on = if n.on { pal.accent } else { pal.muted };
    p.fill((x, cy - 2.0, w * frac, 4.0), 2.0, on, 1.0);
    p.fill((x + w * frac - 6.0, cy - 6.0, 12.0, 12.0), 6.0, pal.fg, 1.0);
    p.right(&format!("{}K", n.warmth), 12.0, Weight::Medium, pal.muted, PANEL_W - PAD, cy);
    p.regions.push(Region::new((x - 6.0, y, w + 12.0, h), 8.0, Action::Warmth));
    y + h + 2.0
}
