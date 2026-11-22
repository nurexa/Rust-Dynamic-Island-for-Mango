//! CPU, GPU, RAM and network tiles. Hardware a machine lacks loses its tile.
use super::{Ctx, PAD};
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

/// One upright meter: its value over it, its name under it.
fn column(p: &Pen, (x, y, w, h): Rect, label: &str, value: &str, frac: f64) {
    let pal = p.pal;
    let mid = x + w / 2.0;
    let vw = p.width(value, 12.5, Weight::Semibold);
    p.text(value, 12.5, Weight::Semibold, pal.fg, mid - vw / 2.0, y + 8.0, -1.0);
    let track = (mid - 7.0, y + 22.0, 14.0, h - 44.0);
    p.well(track, 7.0);
    let full = (track.3 * frac.clamp(0.0, 1.0)).max(14.0);
    p.fill((track.0, track.1 + track.3 - full, 14.0, full), 7.0, pal.accent, 1.0);
    let lw = p.width(label, 10.5, Weight::Semibold);
    p.text(label, 10.5, Weight::Semibold, pal.muted, mid - lw / 2.0, y + h - 8.0, -1.0);
}

/// CPU, GPU and RAM as upright meters with temperatures beside the load, and the
/// network's rates under them.
pub fn panel(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let s = &c.m.sys;
    let pct = |v: f64, t: Option<f64>| match t {
        Some(t) => format!("{v:.0}% {t:.0}°"),
        None => format!("{v:.0}%"),
    };
    let wide = p.w - 2.0 * PAD > 240.0;
    let mut cols: Vec<(&str, String, f64)> = Vec::new();
    if let Some(v) = s.cpu {
        cols.push(("CPU", if wide { pct(v, s.cpu_temp) } else { format!("{v:.0}%") }, v / 100.0));
    }
    if let Some(v) = s.gpu {
        cols.push(("GPU", if wide { pct(v, s.gpu_temp) } else { format!("{v:.0}%") }, v / 100.0));
    }
    if let Some((used, total)) = s.mem {
        cols.push(("RAM", format!("{}G", gib(used)), used / total));
    }
    let (left, w) = (PAD, p.w - 2.0 * PAD);
    let h = 150.0;
    let n = cols.len().max(1) as f64;
    for (i, (label, value, frac)) in cols.iter().enumerate() {
        column(p, (left + i as f64 * w / n, y, w / n, h), label, value, *frac);
    }
    let pal = p.pal;
    let net = |p: &Pen, arrow: &str, v: f64, cy: f64| {
        let text = format!("{arrow} {}", rate(v));
        let tw = p.width(&text, 11.5, Weight::Medium);
        p.text(&text, 11.5, Weight::Medium, pal.muted, left + (w - tw) / 2.0, cy, -1.0);
    };
    net(p, "↓", s.rx, y + h + 12.0);
    net(p, "↑", s.tx, y + h + 28.0);
    y + h + 38.0
}
