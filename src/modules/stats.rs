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

/// One upright meter: its value over it, its name and a detail under it. Heavy load warms
/// the fill toward the error tone; so does a hot chip its temperature.
fn column(p: &Pen, (x, y, w, h): Rect, label: &str, value: &str, frac: f64, detail: (String, bool)) {
    let pal = p.pal;
    let mid = x + w / 2.0;
    let frac = frac.clamp(0.0, 1.0);
    let vw = p.width(value, 13.0, Weight::Bold);
    p.text(value, 13.0, Weight::Bold, pal.fg, mid - vw / 2.0, y + 8.0, -1.0);
    let track = (mid - 8.0, y + 22.0, 16.0, h - 58.0);
    p.well(track, 8.0);
    let heat = ((frac - 0.7) / 0.3).clamp(0.0, 1.0);
    let tone = crate::theme::mix(pal.accent, pal.error, heat);
    let full = (track.3 * frac).max(16.0);
    let top = track.1 + track.3 - full;
    p.fill((track.0, top, 16.0, full), 8.0, tone, 1.0);
    // A soft highlight down the fill's left side, so it reads as a rounded tube.
    p.fill((track.0 + 4.0, top + 4.0, 3.0, (full - 8.0).max(0.0)), 1.5, (1.0, 1.0, 1.0), 0.22);
    // Pinned at the top for long enough, a chip gets a nickname.
    let label = if frac >= 0.95 && label != "RAM" { "toasty!" } else { label };
    let lw = p.width(label, 10.5, Weight::Bold);
    p.text(label, 10.5, Weight::Bold, if heat >= 1.0 { pal.error } else { pal.muted }, mid - lw / 2.0, y + h - 24.0, -1.0);
    let (detail, hot) = detail;
    if !detail.is_empty() {
        let dw = p.width(&detail, 10.5, Weight::Medium);
        p.text(&detail, 10.5, Weight::Medium, if hot { pal.error } else { pal.muted }, mid - dw / 2.0, y + h - 9.0, -1.0);
    }
}

/// CPU, GPU and RAM as upright meters, with temperatures and the RAM there is under
/// them, and the network's rates at the foot.
pub fn panel(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let s = &c.m.sys;
    let temp = |t: Option<f64>| t.map_or((String::new(), false), |t| (format!("{t:.0}°"), t >= 80.0));
    let mut cols: Vec<(&str, String, f64, (String, bool))> = Vec::new();
    if let Some(v) = s.cpu {
        cols.push(("CPU", format!("{v:.0}%"), v / 100.0, temp(s.cpu_temp)));
    }
    if let Some(v) = s.gpu {
        cols.push(("GPU", format!("{v:.0}%"), v / 100.0, temp(s.gpu_temp)));
    }
    if let Some((used, total)) = s.mem {
        cols.push(("RAM", format!("{}G", gib(used)), used / total, (format!("of {:.0}", total / 1073741824.0), false)));
    }
    let (left, w) = (PAD, p.w - 2.0 * PAD);
    let h = 168.0;
    let n = cols.len().max(1) as f64;
    for (i, (label, value, frac, detail)) in cols.into_iter().enumerate() {
        column(p, (left + i as f64 * w / n, y, w / n, h), label, &value, frac, detail);
    }
    let pal = p.pal;
    let text = format!("↓ {}  ↑ {}", rate(s.rx), rate(s.tx));
    let size = if p.width(&text, 11.0, Weight::Medium) > w { 9.5 } else { 11.0 };
    let tw = p.width(&text, size, Weight::Medium);
    p.text(&text, size, Weight::Medium, pal.muted, left + (w - tw) / 2.0, y + h + 10.0, -1.0);
    y + h + 20.0
}
