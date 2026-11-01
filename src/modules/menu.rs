//! A tray app's menu, which takes over the panel while it's open.
use super::{MAX_PANEL_H, PAD, PANEL_W};
use crate::{
    bus::MenuEntry,
    pen::{Action, Pen, Region},
};
use pango::Weight;

pub fn view(p: &mut Pen, title: &str, entries: &[MenuEntry]) -> f64 {
    let pal = p.pal;
    let mut y = 14.0;
    let r = (PAD - 6.0, y, 32.0, 32.0);
    p.button("go-previous", r, false, Region::new(r, 16.0, Action::MenuBack));
    p.text(title, 14.0, Weight::Semibold, pal.fg, PAD + 34.0, y + 16.0, PANEL_W - 2.0 * PAD - 40.0);
    y += 40.0;
    for e in entries {
        if y + 44.0 > MAX_PANEL_H {
            break;
        }
        if e.separator {
            p.fill((PAD, y + 4.0, PANEL_W - 2.0 * PAD, 1.0), 0.5, pal.fg, 0.1);
            y += 9.0;
            continue;
        }
        let rect = (PAD - 6.0, y, PANEL_W - 2.0 * PAD + 12.0, 32.0);
        let cy = y + 16.0;
        let c = if e.enabled { pal.fg } else { pal.muted };
        if e.checked == Some(true) {
            p.symbol("object-select", 14.0, PAD, cy - 7.0, pal.accent);
        }
        let inset = if e.checked.is_some() { 24.0 } else { 4.0 };
        p.text(&e.label, 13.0, Weight::Normal, c, PAD + inset, cy, PANEL_W - 2.0 * PAD - 40.0);
        if !e.children.is_empty() {
            p.symbol("pan-end", 14.0, PANEL_W - PAD - 14.0, cy - 7.0, pal.muted);
        }
        if e.enabled {
            let action = if e.children.is_empty() { Action::MenuClick(e.id) } else { Action::MenuSub(e.id) };
            p.regions.push(Region::new(rect, 10.0, action));
        }
        y += 32.0;
    }
    y + 10.0
}
