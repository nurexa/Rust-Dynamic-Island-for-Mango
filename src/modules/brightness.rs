//! Brightness: the panel's slider. Hidden when there's no backend to read.
use super::{Ctx, PAD, PANEL_W};
use crate::pen::{Action, Pen, Region};
use pango::Weight;

pub fn panel(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let Some(pct) = c.m.brightness else { return y };
    let pal = p.pal;
    let h = 30.0;
    let cy = y + h / 2.0;
    p.symbol("display-brightness", 16.0, PAD, cy - 8.0, pal.fg);
    p.text("Brightness", 12.5, Weight::Medium, pal.fg, PAD + 30.0, cy, 168.0);
    let x = PAD + 30.0 + 168.0 + 12.0;
    let w = PANEL_W - PAD - 40.0 - x;
    p.fill((x, cy - 2.0, w, 4.0), 2.0, pal.fg, 0.14);
    let frac = pct.min(100) as f64 / 100.0;
    p.fill((x, cy - 2.0, w * frac, 4.0), 2.0, pal.accent, 1.0);
    p.fill((x + w * frac - 6.0, cy - 6.0, 12.0, 12.0), 6.0, pal.fg, 1.0);
    p.right(&format!("{pct}%"), 12.0, Weight::Medium, pal.muted, PANEL_W - PAD, cy);
    p.regions.push(Region::new((x - 6.0, y, w + 12.0, h), 8.0, Action::Brightness));
    y + h + 2.0
}
