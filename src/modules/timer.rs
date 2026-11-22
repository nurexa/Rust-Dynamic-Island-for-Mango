//! The pomodoro on the pill (only while one runs) and its row in the panel.
use super::{Ctx, PAD, PILL_H};
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

pub fn panel(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let pal = p.pal;
    let (h, now) = (36.0, Instant::now());
    let cy = y + h / 2.0;
    let on = c.m.timer.is_some();
    p.icon("timer", on, 16.0, PAD, cy - 8.0, if on { pal.accent } else { pal.fg }, 1.0);
    let right = p.w - PAD;
    let btn = |i: f64| (right + 4.0 - (i + 1.0) * 34.0, cy - 16.0, 32.0, 32.0);
    let Some(t) = &c.m.timer else {
        let w = p.text("Pomodoro", 12.5, Weight::Medium, pal.fg, PAD + 30.0, cy, -1.0);
        p.text(&format!("{} min focus", c.cfg.pomodoro.focus), 12.0, Weight::Medium, pal.muted, PAD + 30.0 + w + 10.0, cy, -1.0);
        let r = btn(0.0);
        p.button("play_arrow", r, false, Region::new(r, 16.0, Action::Cmd("timer start".into())).tip("Start focusing"));
        return y + h + 8.0;
    };
    let label = match t.phase {
        crate::timer::Phase::Focus => format!("Focus · {} of {}", t.round, c.cfg.pomodoro.rounds.max(1)),
        phase => phase.label().to_owned(),
    };
    let w = p.text(&label, 12.5, Weight::Medium, pal.fg, PAD + 30.0, cy, 200.0);
    p.text(&t.clock(now), 15.0, Weight::Semibold, if t.paused() { pal.muted } else { pal.fg }, PAD + 30.0 + w + 12.0, cy, -1.0);
    let (play, tip) = if t.paused() { ("play_arrow", "Resume") } else { ("pause", "Pause") };
    for (i, (sym, cmd, tip)) in [("stop", "timer stop", "Stop"), ("skip_next", "timer skip", "Skip to the next phase"),
                                  (play, "timer toggle", tip)].into_iter().enumerate() {
        let r = btn(i as f64);
        p.button(sym, r, false, Region::new(r, 16.0, Action::Cmd(cmd.into())).tip(tip));
    }
    // Progress under the row.
    let bar = p.w - 2.0 * PAD;
    p.meter((PAD, y + h + 1.0, bar, 5.0), t.progress(now));
    y + h + 14.0
}
