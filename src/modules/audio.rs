//! Volume: the panel's slider, and the brief flash on the pill when it changes.
use super::{Ctx, PAD, PANEL_W, PILL_H};
use crate::pen::{Action, Pen, Region, Scroll};
use pango::Weight;

fn symbol(volume: u32, muted: bool) -> &'static str {
    match volume {
        _ if muted => "audio-volume-muted",
        0..=33 => "audio-volume-low",
        34..=66 => "audio-volume-medium",
        _ => "audio-volume-high",
    }
}

/// Replaces the pill for a moment after a volume change. Returns the pill width.
pub fn flash(p: &mut Pen, c: &Ctx) -> Option<f64> {
    let a = c.m.audio.as_ref()?;
    let pal = p.pal;
    let cy = PILL_H / 2.0;
    p.symbol(symbol(a.volume, a.muted), 16.0, 16.0, cy - 8.0, pal.fg);
    let bar = (42.0, cy - 2.0, 104.0, 4.0);
    p.fill(bar, 2.0, pal.fg, 0.18);
    let on = if a.muted { pal.muted } else { pal.accent };
    p.fill((bar.0, bar.1, bar.2 * (a.volume.min(100) as f64 / 100.0), bar.3), 2.0, on, 1.0);
    p.right(&a.volume.to_string(), 13.0, Weight::Semibold, pal.fg, 184.0, cy);
    Some(200.0)
}

pub fn panel(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let Some(a) = &c.m.audio else { return y };
    let pal = p.pal;
    let h = 30.0;
    let cy = y + h / 2.0;
    let mute = (PAD - 6.0, y, 30.0, h);
    p.button(symbol(a.volume, a.muted), mute, a.muted, Region::new(mute, 15.0, Action::Mute).right(Action::Sh(c.cfg.commands.mixer.clone())).tip("Mute"));
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
    let mut slider = Region::new((x - 6.0, y, w + 12.0, h), 8.0, Action::Volume).right(Action::Sh(c.cfg.commands.mixer.clone()));
    slider.scroll = Some(Scroll::Volume);
    p.regions.push(slider);
    y + h + 2.0
}
