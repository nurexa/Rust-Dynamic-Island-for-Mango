//! Notifications on screen: the card the pill grows into, and the history in the panel.
use super::{Ctx, PAD};
use crate::{
    icons,
    notify::{INLINE_REPLY, Note, NoteImage},
    pen::{Action, Pen, Rect, Region},
};
use pango::Weight;

pub const TOAST_W: f64 = 420.0;
const IMAGE: f64 = 44.0;
/// Apps shown in the history, and notes in an expanded one.
const GROUPS: usize = 3;
const EXPANDED: usize = 4;

fn image(p: &mut Pen, note: &Note, x: f64, y: f64, size: f64) -> bool {
    let px = (size * p.scale as f64) as i32;
    if let NoteImage::Named(name) = &note.image {
        if let Some(name) = icons::builtin(name) {
            p.fill((x, y, size, size), size * 0.3, p.pal.accent, 0.18);
            p.icon(name, true, size * 0.55, x + size * 0.225, y + size * 0.225, p.pal.accent, 1.0);
            return true;
        }
    }
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

/// The inline reply: the palette's caret and hint, in a field of its own.
fn field(p: &mut Pen, c: &Ctx, note: &Note, rect: Rect) -> bool {
    let Some(r) = c.m.notes.replying.as_ref().filter(|r| r.id == note.id) else { return false };
    let pal = p.pal;
    p.fill(rect, rect.3 / 2.0, pal.fg, 0.08);
    let (x, cy, room) = (rect.0 + 14.0, rect.1 + rect.3 / 2.0, rect.2 - 28.0);
    let caret = |p: &Pen, at: f64| p.fill((at, cy - 8.0, 2.0, 16.0), 1.0, pal.accent, 1.0);
    if r.text.is_empty() {
        let hint = if note.placeholder.is_empty() { "Write a reply" } else { &note.placeholder };
        p.text(hint, 12.5, Weight::Normal, pal.muted, x + 4.0, cy, room);
        caret(p, x);
    } else {
        // Long replies scroll: the end, where the caret is, stays in view.
        let mut shown = r.text.as_str();
        let mut w = p.width(shown, 12.5, Weight::Medium);
        while w > room {
            let n = shown.chars().count();
            let keep = ((n as f64 * room / w) as usize).min(n - 1);
            shown = &shown[shown.char_indices().nth(n - keep).map_or(shown.len(), |(i, _)| i)..];
            w = p.width(shown, 12.5, Weight::Medium);
        }
        p.text(shown, 12.5, Weight::Medium, pal.fg, x, cy, -1.0);
        caret(p, x + w + 2.0);
    }
    // Clicks in the field stay in it rather than reaching the card behind.
    p.regions.push(Region::new(rect, rect.3 / 2.0, Action::None));
    true
}

/// The card shown for a new notification. Returns its height.
pub fn toast(p: &mut Pen, c: &Ctx) -> f64 {
    let Some(t) = &c.m.notes.showing else { return 64.0 };
    let note = &t.note;
    let pal = p.pal;
    let pad = 16.0;
    let has_image = image(p, note, pad, pad, IMAGE);
    let x = if has_image { pad + IMAGE + 12.0 } else { pad + 4.0 };
    let w = TOAST_W - x - pad - 22.0;
    let tone = if note.urgency == 2 { pal.error } else { pal.fg };
    let title = if note.title.is_empty() { &note.app } else { &note.title };
    // Earlier notes from the same burst, folded into this one.
    let mut title_w = w;
    if t.more > 0 {
        let more = format!("+{}", t.more);
        let bw = p.width(&more, 11.0, Weight::Semibold) + 12.0;
        let bx = TOAST_W - pad - 26.0 - bw;
        p.fill((bx, pad, bw, 18.0), 9.0, pal.accent, 0.2);
        p.text(&more, 11.0, Weight::Semibold, pal.accent, bx + 6.0, pad + 9.0, -1.0);
        title_w = bx - 8.0 - x;
    }
    p.text(title, 13.5, Weight::Semibold, tone, x, pad + 9.0, title_w);
    let body_h = p.wrapped(&note.body, 12.0, pal.muted, x, pad + 20.0, w + 22.0, 3);

    let mut y = (pad + 20.0 + body_h).max(if has_image { pad + IMAGE } else { 0.0 }) + 12.0;
    let close = (TOAST_W - pad - 20.0, pad - 2.0, 22.0, 22.0);
    let default = note.actions.iter().any(|(k, _)| k == "default");
    let click = if default { Action::NoteAction(note.id, "default".into()) } else { Action::NoteDismiss(note.id) };
    p.symbol("close", 12.0, close.0 + 5.0, close.1 + 5.0, pal.muted);
    p.regions.push(Region::new(close, 11.0, Action::NoteDismiss(note.id)));

    let replying = field(p, c, note, (pad, y, TOAST_W - 2.0 * pad, 34.0));
    let buttons: Vec<_> = note.actions.iter().filter(|(k, _)| k != "default").take(3).collect();
    if replying {
        y += 34.0 + 12.0;
    } else if !buttons.is_empty() {
        let mut bx = pad;
        for (key, label) in buttons {
            let w = p.width(label, 12.0, Weight::Medium) + 24.0;
            let rect = (bx, y, w, 28.0);
            p.fill(rect, 14.0, pal.fg, 0.1);
            p.text(label, 12.0, Weight::Medium, pal.fg, bx + 12.0, y + 14.0, -1.0);
            let act = if key == INLINE_REPLY { Action::NoteReply(note.id) } else { Action::NoteAction(note.id, key.clone()) };
            p.regions.push(Region::new(rect, 14.0, act));
            bx += w + 6.0;
        }
        y += 28.0 + 12.0;
    }
    let h = y + 2.0;
    // All of the card, beneath its buttons, and only known once they're laid out.
    p.regions.insert(0, Region::new((0.0, 0.0, TOAST_W, h), 30.0, click).right(Action::NoteDismiss(note.id)));
    h
}

/// History in the panel, one group per app. Takes no room while there's nothing in it.
pub fn panel(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    if !c.cfg.notifications || c.m.notes.history.is_empty() {
        return y;
    }
    history(p, c, y, GROUPS, EXPANDED)
}

/// All of it, scrolling with the sidebar.
pub fn sidebar(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    if !c.cfg.notifications {
        return y;
    }
    if !c.m.notes.history.is_empty() {
        return history(p, c, y, usize::MAX, usize::MAX);
    }
    let pal = p.pal;
    p.text("Notifications", 11.0, Weight::Semibold, pal.muted, PAD, y + 7.0, -1.0);
    p.icon("notifications", false, 22.0, (p.w - 22.0) / 2.0, y + 24.0, pal.muted, 1.0);
    let none = "All caught up";
    let w = p.width(none, 12.0, Weight::Medium);
    p.text(none, 12.0, Weight::Medium, pal.muted, (p.w - w) / 2.0, y + 60.0, -1.0);
    y + 76.0
}

fn history(p: &mut Pen, c: &Ctx, mut y: f64, groups: usize, expanded: usize) -> f64 {
    let pal = p.pal;
    p.text("Notifications", 11.0, Weight::Semibold, pal.muted, PAD, y + 7.0, -1.0);
    let end = p.w - PAD;
    let clear = p.width("Clear", 11.0, Weight::Medium);
    p.right("Clear", 11.0, Weight::Medium, pal.muted, end, y + 7.0);
    p.regions.push(Region::new((end - clear - 8.0, y - 4.0, clear + 16.0, 22.0), 8.0, Action::NoteClear));
    let dnd = "Do not disturb";
    let dnd_w = p.width(dnd, 11.0, Weight::Medium);
    let dnd_x = end - clear - 18.0 - dnd_w;
    p.text(dnd, 11.0, Weight::Medium, if c.m.notes.dnd { pal.accent } else { pal.muted }, dnd_x, y + 7.0, -1.0);
    p.regions.push(Region::new((dnd_x - 8.0, y - 4.0, dnd_w + 16.0, 22.0), 8.0, Action::Dnd));
    y += 22.0;
    for (app, notes) in c.m.notes.groups().into_iter().take(groups) {
        y = group(p, c, app, &notes, y, expanded);
    }
    y + 8.0
}

/// An app's header (icon, name, count, clear), then its newest note, or all of them when open.
fn group(p: &mut Pen, c: &Ctx, app: &str, notes: &[&Note], mut y: f64, expanded: usize) -> f64 {
    let pal = p.pal;
    let end = p.w - PAD;
    let cy = y + 12.0;
    let open = notes.len() > 1 && c.m.notes.expanded(app);
    p.app(app, 14.0, PAD, cy - 7.0, pal.muted, 1.0);
    let name = if app.is_empty() { "Other" } else { app };
    let mut x = PAD + 22.0;
    x += p.text(name, 11.5, Weight::Semibold, pal.muted, x, cy, p.w / 2.0) + 8.0;
    if notes.len() > 1 {
        let n = notes.len().to_string();
        let bw = p.width(&n, 10.5, Weight::Semibold) + 12.0;
        p.fill((x, cy - 8.0, bw, 16.0), 8.0, pal.accent, 0.2);
        p.text(&n, 10.5, Weight::Semibold, pal.accent, x + 6.0, cy, -1.0);
        x += bw + 4.0;
        p.symbol(if open { "keyboard_arrow_up" } else { "keyboard_arrow_down" }, 12.0, x, cy - 6.0, pal.muted);
        let tip = if open { "Show less" } else { "Show all" };
        p.regions.push(Region::new((PAD - 6.0, y, end - 20.0 - (PAD - 6.0), 24.0), 8.0, Action::NoteGroup(app.into())).tip(tip));
    }
    p.symbol("close", 12.0, end - 13.0, cy - 6.0, pal.muted);
    p.regions.push(Region::new((end - 18.0, cy - 10.0, 22.0, 20.0), 10.0, Action::NoteClearApp(app.into())).tip("Clear"));
    y += 24.0;
    for note in notes.iter().take(if open { expanded } else { 1 }) {
        y = row(p, c, note, y);
    }
    y + 4.0
}

fn row(p: &mut Pen, c: &Ctx, note: &Note, y: f64) -> f64 {
    let pal = p.pal;
    let end = p.w - PAD;
    let h = 38.0;
    // Down to the next row: a click in a gap between them would pin the panel.
    p.regions.push(Region::new((PAD - 6.0, y, p.w - 2.0 * PAD + 12.0, h + 2.0), 10.0, Action::NoteDismiss(note.id)).tip("Dismiss"));
    let shown = image(p, note, PAD, y + 5.0, 28.0);
    let x = if shown { PAD + 38.0 } else { PAD };
    let mut w = end - x;
    let replying = c.m.notes.replying.as_ref().is_some_and(|r| r.id == note.id);
    if let Some(label) = note.reply_label().filter(|_| !replying) {
        let bw = p.width(label, 11.0, Weight::Medium) + 20.0;
        let rect = (end - bw, y + 8.0, bw, 22.0);
        p.fill(rect, 11.0, pal.fg, 0.1);
        p.text(label, 11.0, Weight::Medium, pal.fg, rect.0 + 10.0, rect.1 + 11.0, -1.0);
        p.regions.push(Region::new(rect, 11.0, Action::NoteReply(note.id)));
        w -= bw + 8.0;
    }
    let title = if note.title.is_empty() { &note.app } else { &note.title };
    p.text(title, 12.5, Weight::Semibold, pal.fg, x, y + 11.0, w);
    p.text(&note.body.replace('\n', " "), 11.5, Weight::Normal, pal.muted, x, y + 27.0, w);
    if field(p, c, note, (x - 4.0, y + h + 2.0, end - x + 4.0, 32.0)) {
        return y + h + 38.0;
    }
    y + h + 2.0
}
