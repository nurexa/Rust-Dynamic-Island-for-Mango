//! The search palette as it appears inside the panel. Same two tones as the
//! workspaces: the selected row is a solid accent chip, everything else plain ink.
use super::{Ctx, PAD, PANEL_W};
use crate::{
    palette::VISIBLE_ROWS,
    pen::{Action, Pen, Region},
    theme::Rgb,
    tools::{Icon, Item},
};
use pango::Weight;

const ROW_H: f64 = 44.0;
const LIST_TOP: f64 = 62.0;

fn icon(p: &mut Pen, item: &Item, x: f64, cy: f64, tone: Rgb) {
    let size = 26.0;
    let px = (size * p.scale as f64) as i32;
    let picture = match &item.icon {
        Icon::None => None,
        Icon::Glyph(g) => return p.glyph(g, 20.0, x + 3.0, cy - 10.0, tone, 1.0),
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
    p.glyph(entry.glyph(), 18.0, PAD, cy - 9.0, pal.accent, 1.0);
    let x = PAD + 30.0;
    let tabs = if v.ask.is_none() && v.dmenu.is_none() && v.entries.len() > 1 { v.entries.len() } else { 0 };
    let room = PANEL_W - PAD - x - tabs as f64 * 30.0 - 10.0;

    let masked = v.ask.as_ref().is_some_and(|a| a.password);
    let text = if masked { "•".repeat(v.query.chars().count()) } else { v.query.clone() };
    if text.is_empty() {
        let hint = v.ask.as_ref().map_or_else(|| entry.hint(), |a| a.label.clone());
        p.text(&hint, 15.0, Weight::Normal, pal.muted, x, cy, room);
        p.fill((x, cy - 9.0, 2.0, 18.0), 1.0, pal.accent, 1.0);
    } else {
        let w = p.text(&text, 15.0, Weight::Medium, pal.fg, x, cy, room);
        p.fill((x + w + 2.0, cy - 9.0, 2.0, 18.0), 1.0, pal.accent, 1.0);
    }

    let mut tx = PANEL_W - PAD + 2.0 - tabs as f64 * 30.0;
    for (i, e) in v.entries.iter().enumerate().take(tabs) {
        let rect = (tx, cy - 14.0, 28.0, 28.0);
        let on = i == v.current;
        if on {
            p.fill(rect, 10.0, pal.fg, 0.1);
        }
        p.glyph(e.glyph(), 16.0, tx + 6.0, cy - 8.0, if on { pal.fg } else { pal.muted }, 1.0);
        p.regions.push(Region::new(rect, 10.0, Action::PaletteTool(i)).tip(e.hint()));
        tx += 30.0;
    }
}

/// Fixed for a given list, so the panel doesn't breathe with every keystroke.
fn rows(v: &crate::palette::Palette) -> usize {
    v.items.len().clamp(1, VISIBLE_ROWS)
}

pub fn view(p: &mut Pen, c: &Ctx) -> f64 {
    let Some(v) = &c.m.palette else { return 64.0 };
    let pal = p.pal;
    search_row(p, c, 12.0);
    if v.ask.is_some() {
        return 62.0;
    }
    let mut y = LIST_TOP;
    if v.shown.is_empty() {
        let text = match () {
            _ if v.loading => "Loading…".to_owned(),
            _ if v.entries[v.current].typed().is_some() && !v.query.is_empty() => format!("Press Enter to run {}", v.query),
            _ if v.dmenu.is_some() && !v.query.is_empty() => format!("Press Enter to use {}", v.query),
            _ if v.items.is_empty() => "Nothing here yet".to_owned(),
            _ => "No matches".to_owned(),
        };
        p.text(&text, 13.0, Weight::Normal, pal.muted, PAD + 4.0, y + ROW_H / 2.0, PANEL_W - 2.0 * PAD);
    }
    for (row, &index) in v.shown.iter().enumerate().skip(v.top).take(VISIBLE_ROWS) {
        let item = &v.items[index];
        let rect = (PAD - 8.0, y, PANEL_W - 2.0 * PAD + 16.0, ROW_H - 2.0);
        let selected = row == v.selected;
        if selected {
            p.fill(rect, 12.0, pal.accent, 1.0);
        }
        let (fg, dim) = if selected { (pal.on_accent, pal.on_accent) } else { (pal.fg, pal.muted) };
        let cy = y + (ROW_H - 2.0) / 2.0;
        icon(p, item, PAD, cy, fg);
        let x = if item.icon == Icon::None { PAD + 4.0 } else { PAD + 38.0 };
        let w = PANEL_W - PAD - x - 4.0;
        if item.sub.is_empty() {
            p.text(&item.title, 13.5, Weight::Medium, fg, x, cy, w);
        } else {
            p.text(&item.title, 13.5, Weight::Medium, fg, x, cy - 8.0, w);
            p.text(&item.sub, 11.5, Weight::Normal, dim, x, cy + 9.0, w);
        }
        p.regions.push(Region::new(rect, 12.0, Action::PaletteRow(row)));
        y += ROW_H;
    }
    LIST_TOP + rows(v) as f64 * ROW_H + 12.0
}
