//! Brightness: the panel's slider. Hidden when there's no backend to read.
use super::{Ctx, PAD};
use crate::pen::{Action, Pen, Region};
use pango::Weight;

pub fn panel(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    // Desktop monitors over DDC, one row each, under any built-in panel.
    let mut y = y;
    if let Some(pct) = c.m.brightness {
        y = row(p, "Brightness", pct, Action::Brightness, y);
    }
    for s in &c.m.ddc {
        y = row(p, &s.name, s.pct, Action::Ddc(s.bus), y);
    }
    y
}

fn row(p: &mut Pen, label: &str, pct: u32, act: Action, y: f64) -> f64 {
    let pal = p.pal;
    let h = 30.0;
    let cy = y + h / 2.0;
    p.symbol("light_mode", 16.0, PAD, cy - 8.0, pal.fg);
    let label_w = super::label_w(p);
    p.text(label, 12.5, Weight::Medium, pal.fg, PAD + 30.0, cy, label_w);
    let x = PAD + 30.0 + label_w + 12.0;
    let w = p.w - PAD - 40.0 - x;
    p.fill((x, cy - 2.0, w, 4.0), 2.0, pal.fg, 0.14);
    let frac = pct.min(100) as f64 / 100.0;
    p.fill((x, cy - 2.0, w * frac, 4.0), 2.0, pal.accent, 1.0);
    p.fill((x + w * frac - 6.0, cy - 6.0, 12.0, 12.0), 6.0, pal.fg, 1.0);
    p.right(&format!("{pct}%"), 12.0, Weight::Medium, pal.muted, p.w - PAD, cy);
    p.regions.push(Region::new((x - 6.0, y, w + 12.0, h), 8.0, act));
    y + h + 2.0
}
