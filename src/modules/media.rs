//! Now playing, from whichever player playerctld says is current.
use super::{Ctx, PAD, PILL_H};
use crate::{
    pen::{Action, Pen, Rect, Region},
    viz::{self, Live, Look},
};
use pango::Weight;

/// Album art on the pill while something's playing.
pub fn pill(p: &mut Pen, c: &Ctx, x: f64) -> f64 {
    let Some(t) = c.m.media.as_ref().filter(|t| t.playing) else { return x };
    let cy = PILL_H / 2.0;
    let tip = if t.artist.is_empty() { t.title.clone() } else { format!("{} – {}", t.title, t.artist) };
    match &c.m.art_small {
        Some(a) => p.image(a, x, cy - 10.0, 20.0, 1.0),
        None => p.icon("music_note", true, 16.0, x + 2.0, cy - 8.0, p.pal.accent, 1.0),
    }
    let mut end = x + 20.0;
    if c.looks(Look::Bars) {
        p.live.push(Live { look: Look::Bars, rect: (end + 7.0, cy - 8.0, 20.0, 16.0) });
        end += 27.0;
    }
    let sh = |s: &str| Action::Sh(s.to_owned());
    p.regions.push(Region::new((x - 2.0, cy - 12.0, end - x + 4.0, 24.0), 6.0, sh("playerctl play-pause"))
        .right(sh("playerctl next")).middle(sh("playerctl previous")).tip(tip));
    end
}

/// Album art in a square box: round inside a ring when there is one, else a rounded tile.
fn art(p: &mut Pen, c: &Ctx, (x, y, size, _): Rect, playing: bool) {
    let pal = p.pal;
    let ring = c.looks(Look::Ring);
    let d = if ring { size * viz::RING_INNER - 4.0 } else { size };
    let (ax, ay) = (x + (size - d) / 2.0, y + (size - d) / 2.0);
    let r = if ring { d / 2.0 } else { 12.0 };
    match &c.m.art_large {
        Some(a) => {
            p.pill(ax, ay, d, d, r);
            let _ = p.cr.save();
            p.cr.clip();
            p.image(a, ax, ay, d, 1.0);
            let _ = p.cr.restore();
        }
        None => {
            p.well((ax, ay, d, d), r);
            let icon = (d * 0.42).round();
            p.icon("music_note", playing, icon, ax + (d - icon) / 2.0, ay + (d - icon) / 2.0, pal.muted, 1.0);
        }
    }
    if ring && playing {
        p.live.push(Live { look: Look::Ring, rect: (x, y, size, size) });
    }
}

/// The sidebar's player.
pub fn player(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let Some(t) = &c.m.media else { return y };
    let pal = p.pal;
    let (size, width) = (124.0, p.w - 2.0 * PAD);
    // As wide as the cards under it.
    let card = (PAD - 12.0, y, width + 24.0, size + 16.0);
    p.surface(card, 18.0, pal.tint);
    if t.playing && c.looks(Look::Wave) {
        p.live.push(Live { look: Look::Wave, rect: card });
    }
    art(p, c, (PAD + 8.0, y + 8.0, size, size), t.playing);
    let x = PAD + 8.0 + size + 10.0;
    let max = PAD + width - 14.0 - x;
    p.text(&t.title, 14.0, Weight::Semibold, pal.fg, x, y + 30.0, max);
    p.text(&t.artist, 12.0, Weight::Normal, pal.muted, x, y + 49.0, max);
    if t.length > 0.0 {
        p.meter((x, y + 66.0, max, 5.0), t.now() / t.length);
    }
    let (mid, cy) = (x + max / 2.0, y + 103.0);
    let cmd = |s: &str| Action::Sh(s.to_owned());
    let r = (mid - 58.0, cy - 17.0, 34.0, 34.0);
    p.button("skip_previous", r, false, Region::new(r, 17.0, cmd("playerctl previous")));
    let r = (mid - 21.0, cy - 21.0, 42.0, 42.0);
    p.surface(r, 21.0, pal.accent);
    let play = if t.playing { "pause" } else { "play_arrow" };
    p.icon(play, true, 22.0, r.0 + 10.0, r.1 + 10.0, pal.on_accent, 1.0);
    p.regions.push(Region::new(r, 21.0, cmd("playerctl play-pause")));
    let r = (mid + 24.0, cy - 17.0, 34.0, 34.0);
    p.button("skip_next", r, false, Region::new(r, 17.0, cmd("playerctl next")));
    y + card.3
}

pub fn panel(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let Some(t) = &c.m.media else { return y };
    let pal = p.pal;
    let size = 58.0;
    if t.playing && c.looks(Look::Wave) {
        p.live.push(Live { look: Look::Wave, rect: (PAD, y, p.w - 2.0 * PAD, size) });
    }
    if c.looks(Look::Ring) {
        // The ring spills a little into the gaps, so the art stays a decent size.
        art(p, c, (PAD - 6.0, y - 6.0, size + 12.0, size + 12.0), t.playing);
    } else {
        art(p, c, (PAD, y, size, size), t.playing);
    }
    let x = PAD + size + 14.0;
    let max = p.w - PAD - x - 128.0;
    p.text(&t.title, 14.0, Weight::Semibold, pal.fg, x, y + 13.0, max);
    p.text(&t.artist, 12.0, Weight::Normal, pal.muted, x, y + 31.0, max);
    if t.length > 0.0 {
        let bar = (x, y + 46.0, max, 5.0);
        p.meter(bar, t.now() / t.length);
    }
    let cy = y + size / 2.0;
    let end = p.w - PAD;
    let play = if t.playing { "pause" } else { "play_arrow" };
    let cmd = |s: &str| Action::Sh(s.to_owned());
    let r = (end - 118.0, cy - 17.0, 34.0, 34.0);
    p.button("skip_previous", r, false, Region::new(r, 17.0, cmd("playerctl previous")));
    let r = (end - 80.0, cy - 21.0, 42.0, 42.0);
    p.surface(r, 21.0, pal.accent);
    p.icon(play, true, 22.0, r.0 + 10.0, r.1 + 10.0, pal.on_accent, 1.0);
    p.regions.push(Region::new(r, 21.0, cmd("playerctl play-pause")));
    let r = (end - 34.0, cy - 17.0, 34.0, 34.0);
    p.button("skip_next", r, false, Region::new(r, 17.0, cmd("playerctl next")));
    y + size + 16.0
}
