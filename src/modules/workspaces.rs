//! Workspaces and the windows on them. Two tones, like the old Waybar pills: the
//! active workspace is a solid accent chip with its icons in the accent's
//! contrast color, and everything else sits on the background in plain ink.
use super::{Ctx, MAX_COMPACT_W, PAD, PANEL_W, PILL_H};
use crate::{
    Model,
    pen::{Action, Pen, Region},
    wm,
};
use pango::Weight;

const PERSISTENT_TAGS: u32 = 5; // always shown in the panel, so chips don't jump

fn tags_with_windows<'a>(m: &'a Model, mon: &'a wm::Monitor) -> impl Iterator<Item = (&'a wm::Tag, Vec<&'a wm::Client>)> {
    mon.tags.iter().map(|t| (t, wm::clients_on(&m.clients, &mon.name, t.index).collect()))
}


pub fn pill(p: &mut Pen, c: &Ctx, mut x: f64) -> f64 {
    let Some(monitor) = c.m.monitors.iter().find(|m| m.name == c.mon) else { return x };
    let pal = p.pal;
    let cy = PILL_H / 2.0;
    let total = c.m.clients.iter().filter(|w| w.monitor == c.mon && !w.is_minimized).count();
    let max_icons = if total > 12 { 2 } else if total > 7 { 3 } else { 5 };
    for (tag, windows) in tags_with_windows(c.m, monitor) {
        if windows.is_empty() && !tag.is_active {
            continue;
        }
        let label = (tag.index % 10).to_string();
        let digit = p.width(&label, 12.5, Weight::Bold);
        let shown = windows.len().min(max_icons);
        let extra = windows.len() - shown;
        let more = (extra > 0).then(|| format!("+{extra}"));
        let more_w = more.as_ref().map_or(0.0, |s| p.width(s, 11.0, Weight::Medium) + 4.0);
        let w = 8.0 + digit + 8.0 + shown as f64 * 22.0 + more_w + if shown > 0 || more.is_some() { 4.0 } else { 0.0 };
        if x + w + 90.0 > MAX_COMPACT_W {
            break;
        }
        let rect = (x, cy - 12.5, w, 25.0);
        if tag.is_active {
            p.fill(rect, 12.5, pal.accent, 1.0);
        }
        let (digit_c, icon_c) = match () {
            _ if tag.is_urgent => (pal.error, pal.error),
            _ if tag.is_active => (pal.on_accent, pal.on_accent),
            _ => (pal.muted, pal.fg),
        };
        p.text(&label, 12.5, Weight::Bold, digit_c, x + 8.0, cy, -1.0);
        p.regions.push(Region::new(rect, 12.5, Action::View { tag: tag.index, mon: c.mon.into() })
            .right(Action::MoveTo(tag.index)));
        let mut ix = x + 8.0 + digit + 6.0;
        for win in windows.iter().take(shown) {
            let tone = if win.is_urgent { pal.error } else { icon_c };
            p.app(win.appid.as_deref().unwrap_or(""), 18.0, ix, cy - 9.0, tone, 1.0);
            if win.is_focused {
                p.fill((ix + 7.5, cy + 9.5, 3.0, 3.0), 1.5, icon_c, 1.0);
            }
            p.regions.push(Region::new((ix - 2.0, cy - 12.0, 22.0, 24.0), 7.0, Action::Focus(win.id))
                .right(Action::MoveTo(tag.index))
                .tip(win.title.clone().or_else(|| win.appid.clone()).unwrap_or_default()));
            ix += 22.0;
        }
        if let Some(s) = more {
            p.text(&s, 11.0, Weight::Medium, digit_c, ix + 2.0, cy, -1.0);
        }
        x += w + 3.0;
    }
    x - 3.0
}

fn chips(p: &mut Pen, c: &Ctx, mon: &wm::Monitor, mut y: f64) -> f64 {
    let pal = p.pal;
    let (left, right) = (PAD, PANEL_W - PAD);
    let h = 36.0;
    let mut x = left;
    for (tag, windows) in tags_with_windows(c.m, mon) {
        if windows.is_empty() && !tag.is_active && tag.index > PERSISTENT_TAGS {
            continue;
        }
        let label = (tag.index % 10).to_string();
        let digit = p.width(&label, 13.0, Weight::Bold);
        let shown = windows.len().min(6);
        let w = if windows.is_empty() { 36.0 } else { 12.0 + digit + 8.0 + shown as f64 * 26.0 + 8.0 };
        if x + w > right && x > left {
            x = left;
            y += h + 6.0;
        }
        let rect = (x, y, w, h);
        let lead = tag.is_active && mon.active;
        if lead {
            p.fill(rect, 13.0, pal.accent, 1.0);
        } else if tag.is_active {
            p.fill(rect, 13.0, pal.accent, 0.22);
        } else if !windows.is_empty() {
            p.fill(rect, 13.0, pal.fg, 0.07);
        }
        let (digit_c, icon_c) = match () {
            _ if tag.is_urgent => (pal.error, pal.error),
            _ if lead => (pal.on_accent, pal.on_accent),
            _ if tag.is_active => (pal.fg, pal.fg),
            _ if windows.is_empty() => (pal.muted, pal.muted),
            _ => (pal.muted, pal.fg),
        };
        let digit_x = if windows.is_empty() { x + (w - digit) / 2.0 } else { x + 12.0 };
        p.text(&label, 13.0, Weight::Bold, digit_c, digit_x, y + h / 2.0, -1.0);
        p.regions.push(Region::new(rect, 13.0, Action::View { tag: tag.index, mon: mon.name.clone() })
            .right(Action::MoveTo(tag.index)).tip(format!("Workspace {label}")));
        let mut ix = x + 12.0 + digit + 8.0;
        for win in windows.iter().take(shown) {
            let tone = if win.is_urgent { pal.error } else { icon_c };
            p.app(win.appid.as_deref().unwrap_or(""), 20.0, ix, y + 8.0, tone, 1.0);
            if win.is_focused {
                p.fill((ix + 8.5, y + h - 5.0, 3.0, 3.0), 1.5, icon_c, 1.0);
            }
            p.regions.push(Region::new((ix - 3.0, y + 3.0, 26.0, h - 6.0), 8.0, Action::Focus(win.id))
                .right(Action::MoveTo(tag.index))
                .tip(win.title.clone().or_else(|| win.appid.clone()).unwrap_or_default()));
            ix += 26.0;
        }
        x += w + 6.0;
    }
    y + h
}

pub fn panel(p: &mut Pen, c: &Ctx, mut y: f64) -> f64 {
    let pal = p.pal;
    for mon in &c.m.monitors {
        let cy = y + 7.0;
        let mut x = PAD;
        if mon.active {
            p.fill((x + 1.0, cy - 3.0, 6.0, 6.0), 3.0, pal.accent, 1.0);
            x += 13.0;
        }
        let w = p.text(&mon.name, 11.0, Weight::Semibold, pal.muted, x, cy, -1.0);
        p.text(&mon.layout_symbol, 11.0, Weight::Medium, pal.muted, x + w + 8.0, cy, -1.0);
        y = chips(p, c, mon, y + 20.0) + 14.0;
    }
    y
}
