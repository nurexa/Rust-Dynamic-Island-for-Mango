//! Volume: the panel's slider, and the brief flash on the pill when it changes.
use super::{Ctx, PAD, PANEL_W, PILL_H};
use crate::{
    audio,
    pen::{Action, Pen, Region, Scroll},
};
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
    // The device name opens the list of outputs, apps and microphones.
    let w = p.text(&a.device, 12.5, Weight::Medium, pal.fg, PAD + 30.0, cy, label_w - 18.0);
    p.symbol(if c.m.audio_open { "pan-up" } else { "pan-down" }, 12.0, PAD + 33.0 + w, cy - 6.0, pal.muted);
    p.regions.push(Region::new((PAD + 24.0, y + 2.0, w + 30.0, h - 4.0), 8.0, Action::Cmd("audio".into()))
        .right(Action::Sh(c.cfg.commands.mixer.clone())).tip("Outputs, apps and microphones"));
    let x = PAD + 30.0 + label_w + 12.0;
    slider(p, (x, y, PANEL_W - PAD - 40.0 - x, h), a.volume, a.muted, Action::Volume);
    p.right(&format!("{}%", a.volume), 12.0, Weight::Medium, pal.muted, PANEL_W - PAD, cy);
    if let Some(r) = p.regions.last_mut() {
        r.right = Action::Sh(c.cfg.commands.mixer.clone());
        r.scroll = Some(Scroll::Volume);
    }
    let mut y = y + h + 2.0;
    if c.m.audio_open {
        y = devices(p, "Output", &a.sinks, &a.sink, false, y);
        if !a.apps.is_empty() {
            y = heading(p, "Apps", y);
            for s in &a.apps {
                let cy = y + 13.0;
                p.app(&s.icon, 16.0, PAD + 7.0, cy - 8.0, pal.fg, if s.muted { 0.45 } else { 1.0 });
                let r = (PAD + 2.0, y, 26.0, 26.0);
                p.regions.push(Region::new(r, 8.0, Action::StreamMute(s.index)).tip(if s.muted { "Unmute" } else { "Mute" }));
                p.text(&s.app, 12.0, Weight::Medium, if s.muted { pal.muted } else { pal.fg }, PAD + 30.0, cy, label_w);
                let x = PAD + 30.0 + label_w + 12.0;
                slider(p, (x, y, PANEL_W - PAD - 40.0 - x, 26.0), s.volume, s.muted, Action::StreamVolume(s.index));
                p.right(&format!("{}%", s.volume), 11.5, Weight::Medium, pal.muted, PANEL_W - PAD, cy);
                y += 28.0;
            }
        }
        if !a.sources.is_empty() {
            y = devices(p, "Microphone", &a.sources, &a.source, true, y);
        }
        y += 4.0;
    }
    y
}

fn heading(p: &mut Pen, title: &str, y: f64) -> f64 {
    p.text(title, 11.0, Weight::Semibold, p.pal.muted, PAD, y + 9.0, -1.0);
    y + 20.0
}

/// One row per device, the default one marked; a click makes it the default.
fn devices(p: &mut Pen, title: &str, list: &[audio::Device], current: &str, source: bool, y: f64) -> f64 {
    let pal = p.pal;
    let mut y = heading(p, title, y);
    for d in list {
        let cy = y + 13.0;
        let on = d.name == current;
        p.fill((PAD + 9.0, cy - 6.0, 12.0, 12.0), 6.0, if on { pal.accent } else { pal.fg }, if on { 1.0 } else { 0.16 });
        p.text(&d.label, 12.0, Weight::Medium, if on { pal.fg } else { pal.muted }, PAD + 30.0, cy, PANEL_W - 2.0 * PAD - 40.0);
        p.regions.push(Region::new((PAD - 6.0, y, PANEL_W - 2.0 * PAD + 12.0, 26.0), 8.0, Action::AudioDevice { source, name: d.name.clone() }));
        y += 27.0;
    }
    y + 2.0
}

/// A volume bar with its knob; the region it pushes is what dragging moves.
fn slider(p: &mut Pen, (x, y, w, h): (f64, f64, f64, f64), volume: u32, muted: bool, act: Action) {
    let pal = p.pal;
    let cy = y + h / 2.0;
    p.fill((x, cy - 2.0, w, 4.0), 2.0, pal.fg, 0.14);
    let frac = volume.min(100) as f64 / 100.0;
    p.fill((x, cy - 2.0, w * frac, 4.0), 2.0, if muted { pal.muted } else { pal.accent }, 1.0);
    p.fill((x + w * frac - 6.0, cy - 6.0, 12.0, 12.0), 6.0, pal.fg, 1.0);
    p.regions.push(Region::new((x - 6.0, y, w + 12.0, h), 8.0, act));
}
