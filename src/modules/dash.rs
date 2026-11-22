//! The dashboard's small tiles: a big clock, the weather and what this machine is.
//! Each says nothing when it has nothing, so its row gives its share to the rest.
use super::{Ctx, PAD, clock};
use crate::{pen::Pen, sys};
use pango::Weight;

/// Hours over minutes, the minutes in the accent, and the day under them.
pub fn clock(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let pal = p.pal;
    let (h, m) = clock::stacked(&c.cfg.clock);
    let mid = p.w / 2.0;
    for (i, (s, tone)) in [(h, pal.fg), (m, pal.accent)].into_iter().enumerate() {
        let w = p.width(&s, 40.0, Weight::Bold);
        p.text(&s, 40.0, Weight::Bold, tone, mid - w / 2.0, y + 26.0 + i as f64 * 46.0, -1.0);
    }
    let day = clock::format("%a %-e");
    let w = p.width(&day, 12.0, Weight::Semibold);
    p.text(&day, 12.0, Weight::Semibold, pal.muted, mid - w / 2.0, y + 112.0, -1.0);
    y + 126.0
}

pub fn weather(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let Some(w) = &c.m.weather else { return y };
    let pal = p.pal;
    p.icon(w.icon(), true, 40.0, PAD, y + 4.0, pal.accent, 1.0);
    p.text(&format!("{:.0}°", w.temp), 28.0, Weight::Bold, pal.fg, PAD + 52.0, y + 24.0, -1.0);
    p.text(w.text(), 12.5, Weight::Semibold, pal.fg, PAD, y + 62.0, p.w - 2.0 * PAD);
    let range = format!("↑{:.0}° ↓{:.0}°", w.high, w.low);
    let range = if w.place.is_empty() { range } else { format!("{range} · {}", w.place) };
    p.text(&range, 11.5, Weight::Medium, pal.muted, PAD, y + 80.0, p.w - 2.0 * PAD);
    y + 92.0
}

/// Who, on what, for how long.
pub fn about(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let pal = p.pal;
    let a = sys::about();
    let up = c.m.sys.uptime.map(|s| format!("up {}", sys::uptime_text(s)));
    let rows = [("computer", Some(&a.distro).filter(|s| !s.is_empty()).cloned()),
                ("window", Some(c.m.wm).filter(|s| !s.is_empty()).map(str::to_owned)),
                ("terminal", Some(&a.kernel).filter(|s| !s.is_empty()).cloned()),
                ("schedule", up)];
    let mut at = y;
    if !a.user.is_empty() {
        let who = if a.host.is_empty() { a.user.clone() } else { format!("{}@{}", a.user, a.host) };
        p.text(&who, 13.5, Weight::Bold, pal.fg, PAD, at + 10.0, p.w - 2.0 * PAD);
        at += 28.0;
    }
    for (icon, text) in rows.iter().filter_map(|(i, t)| Some((*i, t.as_ref()?))) {
        p.icon(icon, true, 16.0, PAD, at + 3.0, pal.accent, 1.0);
        p.text(text, 12.5, Weight::Medium, pal.fg, PAD + 26.0, at + 11.0, p.w - 2.0 * PAD - 26.0);
        at += 26.0;
    }
    if at == y { y } else { at - 4.0 }
}
