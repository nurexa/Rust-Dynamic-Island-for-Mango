//! Bluetooth: an on/off switch, then what's connected with its battery. Hidden
//! without an adapter.
use super::{Ctx, PAD};
use crate::{
    bluetooth,
    pen::{Action, Pen, Region},
};
use pango::Weight;

fn symbol(icon: &str) -> &'static str {
    match icon {
        "audio-headset" => "headset_mic",
        "audio-headphones" => "headphones",
        "audio-card" | "audio-speakers" => "speaker",
        "input-keyboard" => "keyboard",
        "input-mouse" | "input-tablet" => "mouse",
        "input-gaming" => "sports_esports",
        "phone" => "phone",
        "computer" => "computer",
        _ => "bluetooth",
    }
}

pub fn panel(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let Some(bt) = &c.m.bluetooth else { return y };
    let pal = p.pal;
    let h = 30.0;
    let cy = y + h / 2.0;
    p.icon(if bt.powered { "bluetooth" } else { "bluetooth_disabled" }, bt.powered, 16.0, PAD + 1.0, cy - 8.0, pal.fg, 1.0);
    p.text("Bluetooth", 12.5, Weight::Medium, pal.fg, PAD + 30.0, cy, 168.0);
    let sw = (p.w - PAD - 34.0, cy - 9.0, 34.0, 18.0);
    let (track, alpha, knob) = if bt.powered { (pal.accent, 1.0, pal.on_accent) } else { (pal.fg, 0.18, pal.fg) };
    p.fill(sw, 9.0, track, alpha);
    let kx = if bt.powered { sw.0 + sw.2 - 16.0 } else { sw.0 + 2.0 };
    p.fill((kx, sw.1 + 2.0, 14.0, 14.0), 7.0, knob, 1.0);
    if c.cfg.tools.iter().any(|t| t == "bluetooth") {
        p.regions.push(Region::new((PAD - 6.0, y, sw.0 - PAD - 6.0, h), 10.0, Action::Open("bluetooth")).tip("Devices"));
    }
    let tip = if bt.powered { "Turn Bluetooth off" } else { "Turn Bluetooth on" };
    p.regions.push(Region::new((sw.0 - 6.0, y, sw.2 + 12.0, h), 10.0, Action::Bluetooth).tip(tip));
    let mut y = y + h;
    for d in bt.devices.iter().filter(|d| d.connected) {
        let cy = y + 12.0;
        p.symbol(symbol(&d.icon), 14.0, PAD + 30.0, cy - 7.0, pal.muted);
        p.text(&d.name, 12.0, Weight::Medium, pal.fg, PAD + 52.0, cy, 320.0);
        if let Some(b) = d.battery {
            let tone = if b < bluetooth::LOW { pal.error } else { pal.muted };
            p.right(&format!("{b}%"), 12.0, Weight::Medium, tone, p.w - PAD, cy);
        }
        y += 24.0;
    }
    y + 12.0
}
