//! Claude Code sessions on the pill: working, waiting on you, or just done.
//! The most urgent one shows; a click jumps to its terminal.
use super::{Ctx, PILL_H};
use crate::{
    claude::{self, State},
    pen::{Action, Pen, Region},
};
use pango::Weight;
use std::time::Instant;

const SPARKLE: &str = "\u{f0674}";

pub fn pill(p: &mut Pen, c: &Ctx, x: f64) -> f64 {
    let now = Instant::now();
    let live: Vec<&claude::Session> = c.m.claude.iter().filter(|s| claude::shown(s, now)).collect();
    let Some(top) = live.iter().max_by_key(|s| (s.state == State::Waiting, s.state == State::Working, s.since)) else { return x };
    let pal = p.pal;
    let (label, tone) = match top.state {
        State::Waiting => ("Needs you", pal.error),
        State::Working => ("Working", pal.accent),
        _ => ("Done", pal.fg),
    };
    let cy = PILL_H / 2.0;
    p.glyph(SPARKLE, 16.0, x, cy - 8.0, tone, 1.0);
    let mut w = 20.0 + p.text(label, 12.5, Weight::Semibold, tone, x + 20.0, cy, -1.0);
    if live.len() > 1 {
        w += 6.0 + p.text(&format!("+{}", live.len() - 1), 11.5, Weight::Medium, pal.muted, x + w + 6.0, cy, -1.0);
    }
    let tip = live.iter().map(|s| format!("{}: {}", s.project, match s.state {
        State::Waiting => "needs you", State::Working => "working", _ => "done" })).collect::<Vec<_>>().join(" · ");
    p.regions.push(Region::new((x - 4.0, 4.0, w + 8.0, PILL_H - 8.0), 10.0, Action::Cmd("claude focus".into())).tip(tip));
    x + w
}
