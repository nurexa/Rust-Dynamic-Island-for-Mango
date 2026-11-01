//! Now playing, from whichever player playerctld says is current.
use super::{Ctx, PAD, PANEL_W, PILL_H};
use crate::pen::{Action, Pen, Region};
use pango::Weight;

/// Album art on the pill while something's playing.
pub fn pill(p: &mut Pen, c: &Ctx, x: f64) -> f64 {
    let Some(t) = c.m.media.as_ref().filter(|t| t.playing) else { return x };
    let cy = PILL_H / 2.0;
    let tip = if t.artist.is_empty() { t.title.clone() } else { format!("{} – {}", t.title, t.artist) };
    match &c.m.art_small {
        Some(a) => p.image(a, x, cy - 10.0, 20.0, 1.0),
        None => p.symbol("audio-x-generic", 16.0, x + 2.0, cy - 8.0, p.pal.accent),
    }
    let sh = |s: &str| Action::Sh(s.to_owned());
    p.regions.push(Region::new((x - 2.0, cy - 12.0, 24.0, 24.0), 6.0, sh("playerctl play-pause"))
        .right(sh("playerctl next")).middle(sh("playerctl previous")).tip(tip));
    x + 20.0
}

pub fn panel(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let Some(t) = &c.m.media else { return y };
    let pal = p.pal;
    let size = 58.0;
    match &c.m.art_large {
        Some(a) => {
            p.pill(PAD, y, size, size, 12.0);
            let _ = p.cr.save();
            let _ = p.cr.clip();
            p.image(a, PAD, y, size, 1.0);
            let _ = p.cr.restore();
        }
        None => {
            p.fill((PAD, y, size, size), 12.0, pal.fg, 0.07);
            p.symbol("audio-x-generic", 24.0, PAD + 17.0, y + 17.0, pal.muted);
        }
    }
    let x = PAD + size + 14.0;
    let max = PANEL_W - PAD - x - 128.0;
    p.text(&t.title, 14.0, Weight::Semibold, pal.fg, x, y + 13.0, max);
    p.text(&t.artist, 12.0, Weight::Normal, pal.muted, x, y + 31.0, max);
    if t.length > 0.0 {
        let bar = (x, y + 47.0, max, 3.0);
        p.fill(bar, 1.5, pal.fg, 0.14);
        p.fill((bar.0, bar.1, bar.2 * (t.now() / t.length).clamp(0.0, 1.0), bar.3), 1.5, pal.accent, 1.0);
    }
    let cy = y + size / 2.0;
    let end = PANEL_W - PAD;
    let play = if t.playing { "media-playback-pause" } else { "media-playback-start" };
    let cmd = |s: &str| Action::Sh(s.to_owned());
    let r = (end - 118.0, cy - 17.0, 34.0, 34.0);
    p.button("media-skip-backward", r, false, Region::new(r, 17.0, cmd("playerctl previous")));
    let r = (end - 80.0, cy - 21.0, 42.0, 42.0);
    p.fill(r, 21.0, pal.fg, 0.1);
    p.button(play, r, false, Region::new(r, 21.0, cmd("playerctl play-pause")));
    let r = (end - 34.0, cy - 17.0, 34.0, 34.0);
    p.button("media-skip-forward", r, false, Region::new(r, 17.0, cmd("playerctl next")));
    y + size + 16.0
}
