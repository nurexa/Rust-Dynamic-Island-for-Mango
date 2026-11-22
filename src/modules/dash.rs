//! The dashboard's small tiles: a big clock, the weather and what this machine is.
//! Each says nothing when it has nothing, so its row gives its share to the rest.
use super::{Ctx, PAD, clock};
use crate::{pen::Pen, sys};
use pango::Weight;

/// Hours over minutes, the minutes in the accent; the header has the day. A wish's worth
/// of sparkle at 11:11 and friends, and a cat: humming along to music, asleep after midnight.
pub fn clock(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let pal = p.pal;
    let (h, m) = clock::stacked(&c.cfg.clock);
    let h = if clock::twelve_hour(&c.cfg.clock) { h.trim_start_matches('0').to_owned() } else { h };
    let mid = p.w / 2.0;
    for (i, (s, tone)) in [(&h, pal.fg), (&m, pal.accent)].into_iter().enumerate() {
        let w = p.width(s, 40.0, Weight::Bold);
        p.text(s, 40.0, Weight::Bold, tone, mid - w / 2.0, y + 26.0 + i as f64 * 46.0, -1.0);
    }
    if wish(&h, &m) {
        let w = p.width(&m, 40.0, Weight::Bold);
        p.icon("star_shine", true, 14.0, mid + w / 2.0 - 2.0, y + 50.0, pal.accent, 1.0);
    }
    let playing = c.m.media.as_ref().is_some_and(|t| t.playing);
    let night = clock::format("%H").parse::<u32>().is_ok_and(|h| h < 5);
    if !playing && !night {
        return y + 98.0;
    }
    cat(p, mid, y + 122.0, playing);
    y + 140.0
}

/// 11:11, 12:34, 22:22: times worth a wish.
fn wish(h: &str, m: &str) -> bool {
    let t = format!("{h}{m}");
    let same = t.chars().all(|d| Some(d) == t.chars().next());
    let climbing = t.as_bytes().windows(2).all(|w| w[1] == w[0] + 1);
    t.len() >= 3 && (same || climbing || h == m)
}

/// A small cat's face: eyes closed in a smile with a note by it while music plays,
/// asleep with a z otherwise.
fn cat(p: &mut Pen, cx: f64, cy: f64, happy: bool) {
    let pal = p.pal;
    let cr = &p.cr;
    let (w, h) = (30.0, 22.0);
    p.set(pal.fg, 0.92);
    for side in [-1.0, 1.0] {
        cr.move_to(cx + side * w * 0.46, cy - h * 0.15);
        cr.line_to(cx + side * w * 0.38, cy - h * 0.82);
        cr.line_to(cx + side * w * 0.1, cy - h * 0.42);
        cr.close_path();
        let _ = cr.fill();
    }
    p.fill((cx - w / 2.0, cy - h / 2.0, w, h), 10.0, pal.fg, 0.92);
    p.set(pal.bg, 1.0);
    cr.set_line_width(1.6);
    cr.set_line_cap(cairo::LineCap::Round);
    for side in [-1.0, 1.0] {
        let ex = cx + side * 6.5;
        cr.new_sub_path();
        if happy {
            cr.arc(ex, cy, 2.6, std::f64::consts::PI, 0.0);
        } else {
            cr.move_to(ex - 2.6, cy);
            cr.line_to(ex + 2.6, cy);
        }
        let _ = cr.stroke();
    }
    p.fill((cx - 1.5, cy + 3.0, 3.0, 2.0), 1.0, pal.accent, 1.0);
    if happy {
        p.icon("music_note", true, 12.0, cx + w / 2.0 + 2.0, cy - h * 0.9, pal.accent, 1.0);
    } else {
        p.text("z", 11.0, Weight::Bold, pal.muted, cx + w / 2.0 + 2.0, cy - h * 0.7, -1.0);
        p.text("z", 8.5, Weight::Bold, pal.muted, cx + w / 2.0 + 9.0, cy - h * 1.1, -1.0);
    }
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

#[cfg(test)]
mod tests {
    #[test]
    fn wishes_are_rare() {
        let yes = [("11", "11"), ("12", "34"), ("1", "23"), ("22", "22"), ("3", "33"), ("10", "10")];
        let no = [("4", "57"), ("12", "00"), ("9", "41")];
        assert!(yes.iter().all(|(h, m)| super::wish(h, m)), "{yes:?}");
        assert!(no.iter().all(|(h, m)| !super::wish(h, m)), "{no:?}");
    }
}
