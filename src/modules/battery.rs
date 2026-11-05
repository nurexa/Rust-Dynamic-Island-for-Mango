//! Battery on the pill (laptops only), and the flash when power is plugged or pulled.
use super::{Ctx, PILL_H};
use crate::{
    pen::{Action, Pen, Region},
    power::Battery,
};
use pango::Weight;

pub fn symbol(pct: f64, charging: bool) -> &'static str {
    let level = (pct / 10.0).round() as u32 * 10;
    match (level, charging) {
        (0, false) => "battery-level-0",
        (10, false) => "battery-level-10",
        (20, false) => "battery-level-20",
        (30..=40, false) => "battery-level-40",
        (50..=60, false) => "battery-level-60",
        (70..=80, false) => "battery-level-80",
        (_, false) => "battery-level-100",
        (0..=20, true) => "battery-level-20-charging",
        (30..=60, true) => "battery-level-50-charging",
        (_, true) => "battery-level-80-charging",
    }
}

fn tone(p: &Pen, b: &Battery) -> (f64, f64, f64) {
    if b.pct <= 15.0 && !b.plugged { p.pal.error } else { p.pal.fg }
}

pub fn pill(p: &mut Pen, c: &Ctx, x: f64) -> f64 {
    let Some(b) = &c.m.battery else { return x };
    let cy = PILL_H / 2.0;
    p.symbol(symbol(b.pct, b.charging), 16.0, x, cy - 8.0, tone(p, b));
    let w = p.text(&format!("{:.0}%", b.pct), 12.5, Weight::Semibold, tone(p, b), x + 19.0, cy, -1.0);
    p.regions.push(Region::new((x - 4.0, 4.0, w + 27.0, PILL_H - 8.0), 10.0, Action::None)
        .tip(b.time().unwrap_or_else(|| if b.plugged { "Plugged in".into() } else { "On battery".into() })));
    x + 19.0 + w
}

/// Replaces the pill for a moment when power is plugged in or pulled out.
pub fn flash(p: &mut Pen, c: &Ctx) -> Option<f64> {
    let b = c.m.battery.as_ref()?;
    let cy = PILL_H / 2.0;
    let on = if b.plugged { p.pal.accent } else { p.pal.fg };
    p.symbol(symbol(b.pct, b.charging), 16.0, 16.0, cy - 8.0, on);
    p.text(if b.plugged { "Charging" } else { "On battery" }, 13.0, Weight::Semibold, p.pal.fg, 40.0, cy, -1.0);
    p.right(&format!("{:.0}%", b.pct), 13.0, Weight::Semibold, on, 184.0, cy);
    Some(200.0)
}
