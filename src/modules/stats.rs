//! CPU, GPU, RAM and network tiles. Hardware a machine lacks loses its tile.
use super::{Ctx, PAD, PANEL_W};
use crate::pen::{Pen, Rect};
use pango::Weight;

fn rate(v: f64) -> String {
    match v {
        v if v >= 1e9 => format!("{:.1} GB/s", v / 1e9),
        v if v >= 1e6 => format!("{:.1} MB/s", v / 1e6),
        v if v >= 1e3 => format!("{:.0} KB/s", v / 1e3),
        v => format!("{v:.0} B/s"),
    }
}

fn gib(v: f64) -> String {
    format!("{:.1}", v / (1024.0 * 1024.0 * 1024.0))
}

fn tile(p: &mut Pen, rect: Rect, label: &str, value: &str, sub: &str, bar: Option<f64>) {
    let pal = p.pal;
    let (x, y, w, h) = rect;
    p.fill(rect, 14.0, pal.fg, 0.055);
    p.text(label, 10.5, Weight::Medium, pal.muted, x + 12.0, y + 15.0, -1.0);
    if !sub.is_empty() {
        p.right(sub, 11.0, Weight::Medium, pal.muted, x + w - 12.0, y + 15.0);
    }
    p.text(value, 17.0, Weight::Semibold, pal.fg, x + 12.0, y + 35.0, w - 24.0);
    let track = (x + 12.0, y + h - 10.0, w - 24.0, 3.0);
    if let Some(frac) = bar {
        p.fill(track, 1.5, pal.fg, 0.1);
        p.fill((track.0, track.1, track.2 * frac.clamp(0.015, 1.0), track.3), 1.5, pal.accent, 1.0);
    }
}

pub fn panel(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let s = &c.m.sys;
    let temp = |t: Option<f64>| t.map_or(String::new(), |t| format!("{t:.0}°"));
    let mut tiles: Vec<(&str, String, String, Option<f64>)> = Vec::new();
    if let Some(c) = s.cpu {
        tiles.push(("CPU", format!("{c:.0}%"), temp(s.cpu_temp), Some(c / 100.0)));
    }
    if let Some(g) = s.gpu {
        tiles.push(("GPU", format!("{g:.0}%"), temp(s.gpu_temp), Some(g / 100.0)));
    }
    if let Some((used, total)) = s.mem {
        tiles.push(("RAM", format!("{} GB", gib(used)), format!("of {}", gib(total)), Some(used / total)));
    }
    tiles.push(("NET", format!("↓ {}", rate(s.rx)), format!("↑ {}", rate(s.tx)), None));
    let n = tiles.len() as f64;
    let gap = 8.0;
    let w = (PANEL_W - 2.0 * PAD - (n - 1.0) * gap) / n;
    let h = 58.0;
    for (i, (label, value, sub, bar)) in tiles.iter().enumerate() {
        tile(p, (PAD + i as f64 * (w + gap), y, w, h), label, value, sub, *bar);
    }
    y + h + 12.0
}
