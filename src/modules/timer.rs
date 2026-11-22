//! The pomodoro on the pill (only while one runs) and its row in the panel.
use super::{Ctx, PAD, PILL_H};
use crate::timer::{Phase, Timer};
use crate::pen::{Action, Pen, Region};
use pango::Weight;
use std::time::Instant;

pub fn pill(p: &mut Pen, c: &Ctx, x: f64) -> f64 {
    let Some(t) = &c.m.timer else { return x };
    let tone = if t.paused() { p.pal.muted } else { p.pal.accent };
    p.icon("timer", !t.paused(), 14.0, x, PILL_H / 2.0 - 7.0, tone, 1.0);
    let w = p.text(&t.short(Instant::now()), 12.5, Weight::Semibold, tone, x + 19.0, PILL_H / 2.0, -1.0);
    p.regions.push(Region::new((x - 4.0, 4.0, w + 27.0, PILL_H - 8.0), 10.0, Action::Cmd("timer toggle".into()))
        .right(Action::Cmd("timer stop".into())).tip(format!("{} · click pauses, right-click stops", t.phase.label())));
    x + 19.0 + w
}

/// A ring around what's left, the phase under it and a dot per round. Click the ring to
/// start or pause, right-click to stop.
pub fn panel(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let pal = p.pal;
    let now = Instant::now();
    let t = c.m.timer.as_ref();
    let (d, mid) = (92.0, p.w / 2.0);
    let (cx, cy) = (mid, y + d / 2.0 + 2.0);
    let cr = &p.cr;
    cr.set_line_width(7.0);
    cr.set_line_cap(cairo::LineCap::Round);
    let ring = d / 2.0 - 4.0;
    cr.new_sub_path();
    cr.arc(cx, cy, ring, 0.0, std::f64::consts::TAU);
    p.set(pal.fg, if pal.dark { 0.1 } else { 0.08 });
    let _ = cr.stroke();
    if let Some(t) = t.filter(|t| t.progress(now) > 0.0) {
        let top = -std::f64::consts::FRAC_PI_2;
        cr.new_sub_path();
        cr.arc(cx, cy, ring, top, top + std::f64::consts::TAU * t.progress(now).min(1.0));
        p.set(if t.paused() { pal.muted } else { pal.accent }, 1.0);
        let _ = cr.stroke();
    }
    let celebrating = t.is_some_and(|t| t.phase == Phase::Long);
    if celebrating {
        tomato(p, cx, cy - 10.0, 13.0);
    }
    let left = t.map_or_else(|| format!("{}:00", c.cfg.pomodoro.focus), |t| t.clock(now));
    let size = if celebrating { 14.0 } else { 20.0 };
    let w = p.width(&left, size, Weight::Bold);
    let tone = if t.is_some_and(Timer::paused) { pal.muted } else { pal.fg };
    p.text(&left, size, Weight::Bold, tone, cx - w / 2.0, if celebrating { cy + 14.0 } else { cy }, -1.0);
    let (action, tip) = match t {
        None => ("timer start", "Start focusing"),
        Some(t) if t.paused() => ("timer toggle", "Resume · right-click stops"),
        Some(_) => ("timer toggle", "Pause · right-click stops"),
    };
    p.regions.push(Region::new((cx - d / 2.0, cy - d / 2.0, d, d), d / 2.0, Action::Cmd(action.into()))
        .right(Action::Cmd("timer stop".into())).tip(tip));

    let label = match t {
        None => "Pomodoro".to_owned(),
        Some(t) if t.phase == Phase::Long => "Well done!".to_owned(),
        Some(t) => t.phase.label().to_owned(),
    };
    let lw = p.width(&label, 12.0, Weight::Semibold);
    let ly = y + d + 16.0;
    p.text(&label, 12.0, Weight::Semibold, pal.fg, mid - lw / 2.0, ly, -1.0);
    // A dot per round: done ones full, the current one ringed.
    let rounds = c.cfg.pomodoro.rounds.clamp(1, 8);
    let round = t.filter(|t| t.phase != Phase::Countdown).map_or(0, |t| t.round);
    let dx = mid - (rounds as f64 * 12.0 - 6.0) / 2.0;
    for i in 1..=rounds {
        let r = (dx + (i - 1) as f64 * 12.0, ly + 12.0, 6.0, 6.0);
        match () {
            _ if i < round || celebrating => p.fill(r, 3.0, pal.accent, 1.0),
            _ if i == round => p.fill(r, 3.0, pal.accent, 0.5),
            _ => p.fill(r, 3.0, pal.fg, 0.16),
        }
    }
    if t.is_some() {
        let b = (p.w - PAD - 22.0, y, 26.0, 26.0);
        p.button("skip_next", b, false, Region::new(b, 13.0, Action::Cmd("timer skip".into())).tip("Skip to the next phase"));
    }
    ly + 26.0
}

/// A little tomato, for finishing a whole set.
fn tomato(p: &Pen, cx: f64, cy: f64, r: f64) {
    let cr = &p.cr;
    cr.new_sub_path();
    cr.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
    p.set((0.93, 0.33, 0.3), 1.0);
    let _ = cr.fill();
    cr.new_sub_path();
    cr.arc(cx - r * 0.38, cy - r * 0.35, r * 0.22, 0.0, std::f64::consts::TAU);
    p.set((1.0, 1.0, 1.0), 0.45);
    let _ = cr.fill();
    p.set((0.36, 0.72, 0.36), 1.0);
    for k in [-1.0, 0.0, 1.0] {
        cr.new_sub_path();
        cr.arc(cx + k * r * 0.32, cy - r * 0.88, r * 0.22, 0.0, std::f64::consts::TAU);
        let _ = cr.fill();
    }
}
