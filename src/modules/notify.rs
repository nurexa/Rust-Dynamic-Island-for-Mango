//! Notifications on screen: the card the pill grows into, and the history in the panel.
use super::{Ctx, PAD, PANEL_W};
use crate::{
    notify::{Note, NoteImage},
    pen::{Action, Pen, Region},
};
use pango::Weight;

pub const TOAST_W: f64 = 420.0;
const IMAGE: f64 = 44.0;

fn image(p: &mut Pen, note: &Note, x: f64, y: f64, size: f64) -> bool {
    let px = (size * p.scale as f64) as i32;
    let img = match &note.image {
        NoteImage::None => None,
        NoteImage::Path(path) => p.icons.picture(path, px),
        NoteImage::Named(name) => p.icons.themed(name, px),
        NoteImage::Pixels { w, h, rgba } => p.icons.pixels(rgba, *w, *h, px),
    };
    let Some(img) = img else { return false };
    p.pill(x, y, size, size, 10.0);
    let _ = p.cr.save();
    let _ = p.cr.clip();
    p.image(&img, x, y, size, 1.0);
    let _ = p.cr.restore();
    true
}

/// The card shown for a new notification. Returns its height.
pub fn toast(p: &mut Pen, c: &Ctx) -> f64 {
    let Some(note) = &c.m.notes.showing else { return 64.0 };
    let pal = p.pal;
    let pad = 16.0;
    let has_image = image(p, note, pad, pad, IMAGE);
    let x = if has_image { pad + IMAGE + 12.0 } else { pad + 4.0 };
    let w = TOAST_W - x - pad - 22.0;
    let tone = if note.urgency == 2 { pal.error } else { pal.fg };
    let title = if note.title.is_empty() { &note.app } else { &note.title };
    p.text(title, 13.5, Weight::Semibold, tone, x, pad + 9.0, w);
    let body_h = p.wrapped(&note.body, 12.0, pal.muted, x, pad + 20.0, w + 22.0, 3);

    let mut y = (pad + 20.0 + body_h).max(if has_image { pad + IMAGE } else { 0.0 }) + 12.0;
    let close = (TOAST_W - pad - 20.0, pad - 2.0, 22.0, 22.0);
    let default = note.actions.iter().any(|(k, _)| k == "default");
    let click = if default { Action::NoteAction(note.id, "default".into()) } else { Action::NoteDismiss(note.id) };
    p.regions.push(Region::new((0.0, 0.0, TOAST_W, y), 18.0, click).right(Action::NoteDismiss(note.id)));
    p.symbol("window-close", 12.0, close.0 + 5.0, close.1 + 5.0, pal.muted);
    p.regions.push(Region::new(close, 11.0, Action::NoteDismiss(note.id)));

    let buttons: Vec<_> = note.actions.iter().filter(|(k, _)| k != "default").take(3).collect();
    if !buttons.is_empty() {
        let mut bx = pad;
        for (key, label) in buttons {
            let w = p.width(label, 12.0, Weight::Medium) + 24.0;
            let rect = (bx, y, w, 28.0);
            p.fill(rect, 14.0, pal.fg, 0.1);
            p.text(label, 12.0, Weight::Medium, pal.fg, bx + 12.0, y + 14.0, -1.0);
            p.regions.push(Region::new(rect, 14.0, Action::NoteAction(note.id, key.clone())));
            bx += w + 6.0;
        }
        y += 28.0 + 12.0;
    }
    y + 2.0
}

/// History in the panel. Takes no room while there's nothing in it.
pub fn panel(p: &mut Pen, c: &Ctx, mut y: f64) -> f64 {
    if !c.cfg.notifications || c.m.notes.history.is_empty() {
        return y;
    }
    let pal = p.pal;
    p.text("Notifications", 11.0, Weight::Semibold, pal.muted, PAD, y + 7.0, -1.0);
    let end = PANEL_W - PAD;
    let clear = p.width("Clear", 11.0, Weight::Medium);
    p.right("Clear", 11.0, Weight::Medium, pal.muted, end, y + 7.0);
    p.regions.push(Region::new((end - clear - 8.0, y - 4.0, clear + 16.0, 22.0), 8.0, Action::NoteClear));
    let dnd = "Do not disturb";
    let dnd_w = p.width(dnd, 11.0, Weight::Medium);
    let dnd_x = end - clear - 18.0 - dnd_w;
    p.text(dnd, 11.0, Weight::Medium, if c.m.notes.dnd { pal.accent } else { pal.muted }, dnd_x, y + 7.0, -1.0);
    p.regions.push(Region::new((dnd_x - 8.0, y - 4.0, dnd_w + 16.0, 22.0), 8.0, Action::Dnd));
    y += 22.0;
    for note in c.m.notes.history.iter().take(3) {
        let h = 38.0;
        let rect = (PAD - 6.0, y, PANEL_W - 2.0 * PAD + 12.0, h);
        let shown = image(p, note, PAD, y + 5.0, 28.0);
        let x = if shown { PAD + 38.0 } else { PAD };
        let w = end - x;
        let title = if note.title.is_empty() { &note.app } else { &note.title };
        p.text(title, 12.5, Weight::Semibold, pal.fg, x, y + 11.0, w);
        p.text(&note.body.replace('\n', " "), 11.5, Weight::Normal, pal.muted, x, y + 27.0, w);
        p.regions.push(Region::new(rect, 10.0, Action::NoteDismiss(note.id)).tip("Dismiss"));
        y += h + 2.0;
    }
    y + 12.0
}
