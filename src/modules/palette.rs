//! The search palette as it appears inside the panel. Same two tones as the
//! workspaces: the selected row is a solid accent chip, everything else plain ink.
use super::{Ctx, PAD};
use crate::{
    palette::VISIBLE_ROWS,
    pen::{Action, Pen, Region},
    theme::Rgb,
    tools::{Icon, Item, Tail},
};
use pango::Weight;

const TAB: f64 = 27.0;
const ROW_H: f64 = 44.0;
const LIST_TOP: f64 = 62.0;

fn icon(p: &mut Pen, item: &Item, x: f64, cy: f64, tone: Rgb, selected: bool) {
    let size = 26.0;
    let px = (size * p.scale) as i32;
    let picture = match &item.icon {
        Icon::None => None,
        Icon::Glyph(g) => return p.mark(g, selected, 20.0, x + 3.0, cy - 10.0, tone, 1.0),
        Icon::Named(name) => p.icons.themed(name, px),
        Icon::Image(path) => p.icons.picture(path, px),
    };
    match picture {
        Some(img) if matches!(item.icon, Icon::Image(_)) => {
            p.pill(x, cy - size / 2.0, size, size, 7.0);
            let _ = p.cr.save();
            let _ = p.cr.clip();
            p.image(&img, x, cy - size / 2.0, size, 1.0);
            let _ = p.cr.restore();
        }
        Some(img) => p.image(&img, x, cy - size / 2.0, size, 1.0),
        None if matches!(item.icon, Icon::Named(_)) => p.letter(&item.title, size, x, cy - size / 2.0, 1.0),
        None => {}
    }
}

fn search_row(p: &mut Pen, c: &Ctx, y: f64) {
    let Some(v) = &c.m.palette else { return };
    let pal = p.pal;
    let cy = y + 18.0;
    let entry = &v.entries[v.current];
    p.mark(entry.glyph(), true, 18.0, PAD, cy - 9.0, pal.accent, 1.0);
    let x = PAD + 30.0;
    let tabs = if v.ask.is_none() && v.dmenu.is_none() && v.entries.len() > 1 { v.entries.len() } else { 0 };
    let room = p.w - PAD - x - tabs as f64 * TAB - 10.0;

    let masked = v.ask.as_ref().is_some_and(|a| a.password);
    let text = if masked { "•".repeat(v.query.chars().count()) } else { v.query.clone() };
    if text.is_empty() {
        // Inside a list, where that is.
        let here = || v.trail.last().map_or_else(|| entry.hint(), |page| page.0.clone());
        let hint = v.ask.as_ref().map_or_else(here, |a| a.label.clone());
        p.text(&hint, 15.0, Weight::Normal, pal.muted, x, cy, room);
        p.fill((x, cy - 9.0, 2.0, 18.0), 1.0, pal.accent, 1.0);
    } else {
        let w = p.text(&text, 15.0, Weight::Medium, pal.fg, x, cy, room);
        p.fill((x + w + 2.0, cy - 9.0, 2.0, 18.0), 1.0, pal.accent, 1.0);
    }

    let mut tx = p.w - PAD + 2.0 - tabs as f64 * TAB;
    for (i, e) in v.entries.iter().enumerate().take(tabs) {
        let rect = (tx, cy - 13.0, 26.0, 26.0);
        let on = i == v.current;
        if on {
            p.surface(rect, 10.0, pal.accent);
        }
        p.mark(e.glyph(), on, 16.0, tx + 5.0, cy - 8.0, if on { pal.on_accent } else { pal.muted }, 1.0);
        p.regions.push(Region::new(rect, 10.0, Action::PaletteTool(i)).tip(e.hint()));
        tx += TAB;
    }
}

/// Fixed for a given list, so the panel doesn't breathe with every keystroke.
fn rows(v: &crate::palette::Palette) -> usize {
    v.items.len().clamp(1, VISIBLE_ROWS)
}

pub fn view(p: &mut Pen, c: &Ctx) -> f64 {
    let Some(v) = &c.m.palette else { return 64.0 };
    let pal = p.pal;
    p.well((PAD - 10.0, 10.0, p.w - 2.0 * PAD + 20.0, 40.0), 16.0);
    search_row(p, c, 12.0);
    if v.ask.is_some() {
        return 62.0 + notice(p, c, 54.0);
    }
    let mut y = LIST_TOP;
    if v.shown.is_empty() {
        let text = match () {
            _ if v.entries[v.current].live() && v.query.trim().chars().count() < 2 => "Type to search".to_owned(),
            _ if v.loading => "Loading…".to_owned(),
            _ if v.entries[v.current].typed().is_some() && !v.query.is_empty() => format!("Press Enter to run {}", v.query),
            _ if v.dmenu.is_some() && !v.query.is_empty() => format!("Press Enter to use {}", v.query),
            _ if v.items.is_empty() => "Nothing here yet".to_owned(),
            _ => "No matches".to_owned(),
        };
        p.text(&text, 13.0, Weight::Normal, pal.muted, PAD + 4.0, y + ROW_H / 2.0, p.w - 2.0 * PAD);
    }
    for (row, &index) in v.shown.iter().enumerate().skip(v.top).take(VISIBLE_ROWS) {
        let item = &v.items[index];
        let rect = (PAD - 8.0, y, p.w - 2.0 * PAD + 16.0, ROW_H - 2.0);
        let selected = row == v.selected;
        if selected {
            p.surface(rect, 12.0, pal.accent);
        }
        let (fg, dim) = if selected { (pal.on_accent, pal.on_accent) } else { (pal.fg, pal.muted) };
        let cy = y + (ROW_H - 2.0) / 2.0;
        icon(p, item, PAD, cy, fg, selected);
        let x = if item.icon == Icon::None { PAD + 4.0 } else { PAD + 38.0 };
        let used = tail(p, &item.tail, p.w - PAD, cy, fg, selected);
        let w = p.w - PAD - x - 4.0 - used;
        if item.sub.is_empty() {
            p.text(&item.title, 13.5, Weight::Medium, fg, x, cy, w);
        } else {
            p.text(&item.title, 13.5, Weight::Medium, fg, x, cy - 8.0, w);
            p.text(&item.sub, 11.5, Weight::Normal, dim, x, cy + 9.0, w);
        }
        p.regions.push(Region::new(rect, 12.0, Action::PaletteRow(row)));
        y += ROW_H;
    }
    let list_h = rows(v) as f64 * ROW_H;
    if v.shown.len() > VISIBLE_ROWS {
        // That there's more, and where in it this is.
        let thumb = (list_h * VISIBLE_ROWS as f64 / v.shown.len() as f64).max(24.0);
        let at = (list_h - thumb) * (v.top as f64 / (v.shown.len() - VISIBLE_ROWS) as f64).min(1.0);
        p.fill((p.w - 9.0, LIST_TOP + at, 3.0, thumb), 1.5, pal.fg, 0.3);
    }
    let bottom = LIST_TOP + list_h;
    bottom + notice(p, c, bottom) + 12.0
}

/// What a row says at its right end. Returns the room it took.
fn tail(p: &mut Pen, tail: &Tail, end: f64, cy: f64, tone: Rgb, selected: bool) -> f64 {
    let pal = p.pal;
    match tail {
        Tail::None => 0.0,
        Tail::Switch(on) if selected => {
            // Accent on accent would hide which way it is.
            let track = (end - 36.0, cy - 10.0, 36.0, 20.0);
            p.fill(track, 10.0, pal.on_accent, if *on { 1.0 } else { 0.25 });
            let knob = if *on { pal.accent } else { pal.on_accent };
            p.fill((end - if *on { 17.0 } else { 33.0 }, cy - 7.0, 14.0, 14.0), 7.0, knob, 1.0);
            48.0
        }
        Tail::Switch(on) => {
            p.switch(end - 36.0, cy, *on);
            48.0
        }
        Tail::Value(value) => {
            let w = (p.width(value, 12.0, Weight::Semibold) + 22.0).clamp(28.0, 190.0);
            let chip = (end - w, cy - 12.0, w, 24.0);
            if selected {
                p.fill(chip, 12.0, pal.on_accent, 0.16);
            } else {
                p.raised(chip, 12.0);
            }
            p.text(value, 12.0, Weight::Semibold, tone, chip.0 + 11.0, cy, w - 20.0);
            w + 12.0
        }
        Tail::More => {
            p.icon("chevron_right", false, 16.0, end - 16.0, cy - 8.0, tone, 1.0);
            28.0
        }
    }
}

/// How the last change went, under the list. Returns the height it took.
fn notice(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let Some((text, ok)) = c.m.palette.as_ref().and_then(|v| v.notice.as_ref()) else { return 0.0 };
    let tone = if *ok { p.pal.accent } else { p.pal.error };
    p.icon(if *ok { "check" } else { "close" }, true, 13.0, PAD + 4.0, y + 8.0, tone, 1.0);
    p.text(text, 11.5, Weight::Medium, tone, PAD + 24.0, y + 14.5, p.w - 2.0 * PAD - 24.0);
    24.0
}
