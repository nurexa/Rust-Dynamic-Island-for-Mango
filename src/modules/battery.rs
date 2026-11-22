//! Battery on the pill (laptops only), and the flash when power is plugged or pulled.
use super::{Ctx, PILL_H};
use crate::{
    pen::{Action, Pen, Region},
    power::Battery,
};
use pango::Weight;

pub fn symbol(pct: f64, charging: bool) -> &'static str {
    match (pct as u32, charging) {
        (_, true) => "battery_charging_full",
        (0..=9, _) => "battery_alert",
        (10..=24, _) => "battery_1_bar",
        (25..=39, _) => "battery_2_bar",
        (40..=54, _) => "battery_3_bar",
        (55..=69, _) => "battery_4_bar",
        (70..=84, _) => "battery_5_bar",
        (85..=94, _) => "battery_6_bar",
        _ => "battery_full",
    }
}

fn tone(p: &Pen, b: &Battery) -> (f64, f64, f64) {
    if b.pct <= 15.0 && !b.plugged { p.pal.error } else { p.pal.fg }
}

pub fn pill(p: &mut Pen, c: &Ctx, x: f64) -> f64 {
    let Some(b) = &c.m.battery else { return x };
    let cy = PILL_H / 2.0;
    p.icon(symbol(b.pct, b.charging), b.charging, 16.0, x, cy - 8.0, tone(p, b), 1.0);
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
