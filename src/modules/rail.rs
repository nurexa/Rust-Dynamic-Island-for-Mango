//! The frame's rail and the bar's two ends: what the island hands over when part
//! of the screen's edge can carry it. Both draw from the same lists as the island,
//! so taking a name out of `pill` or `panel` takes it out of here too.
use super::{Ctx, audio, battery, clock, status, workspaces};
use crate::{
    pen::{Action, Pen, Region, Scroll},
    style::RAIL,
    wm,
};
use pango::Weight;

const ICON: f64 = 18.0;
const BUTTON: f64 = 28.0;
const PERSISTENT_TAGS: u32 = 5; // as in the panel, so dots don't jump
const TAG_ICONS: usize = 3;
const TRAY: usize = 8;

struct Button {
    icon: &'static str,
    fill: bool,
    left: Action,
    right: Action,
    scroll: Option<Scroll>,
}

impl Button {
    fn new(icon: &'static str, fill: bool, left: Action) -> Self {
        Button { icon, fill, left, right: Action::None, scroll: None }
    }

    fn draw(&self, p: &mut Pen, x: f64, y: f64) {
        let inset = (BUTTON - ICON) / 2.0;
        p.icon(self.icon, self.fill, ICON, x + inset, y + inset, p.pal.fg, 1.0);
        let mut r = Region::new((x, y, BUTTON, BUTTON), BUTTON / 2.0, self.left.clone()).right(self.right.clone());
        r.scroll = self.scroll;
        p.regions.push(r);
    }
}

fn has(list: &[String], name: &str) -> bool {
    list.iter().any(|n| n == name)
}

/// Volume, network and Bluetooth: whichever this machine has.
fn state(c: &Ctx) -> Vec<Button> {
    let (m, cfg) = (c.m, c.cfg);
    let mut list = Vec::new();
    if let Some(a) = m.audio.as_ref().filter(|_| has(&cfg.panel, "audio")) {
        let mut b = Button::new(audio::symbol(a.volume, a.muted), a.muted, Action::Mute);
        b.right = Action::Sh(cfg.commands.mixer.clone());
        b.scroll = Some(Scroll::Volume);
        list.push(b);
    }
    if has(&cfg.panel, "status") {
        let open = if has(&cfg.tools, "wifi") { Action::Open("wifi") } else { Action::Sh(cfg.commands.wifi.clone()) };
        let mut b = Button::new(status::net_symbol(&m.net), false, open);
        b.right = Action::Sh(cfg.commands.wifi_alt.clone());
        list.push(b);
    }
    if let Some(bt) = m.bluetooth.as_ref().filter(|_| has(&cfg.panel, "bluetooth")) {
        let icon = if bt.powered { "bluetooth" } else { "bluetooth_disabled" };
        let open = if has(&cfg.tools, "bluetooth") { Action::Open("bluetooth") } else { Action::Bluetooth };
        let mut b = Button::new(icon, bt.powered, open);
        b.right = Action::Bluetooth;
        list.push(b);
    }
    list
}

fn tray(p: &mut Pen, c: &Ctx, i: usize, x: f64, y: f64) {
    let (item, icon) = &c.m.tray[i];
    let at = (BUTTON - ICON) / 2.0;
    match icon {
        Some((img, false)) => p.image(img, x + at, y + at, ICON, 1.0),
        Some((mask, true)) => {
            let s = mask.width() as f64 / ICON;
            mask.set_device_scale(s, s);
            p.set(p.pal.fg, 1.0);
            let _ = p.cr.mask_surface(mask, x + at, y + at);
        }
        None => p.letter(&item.title, ICON, x + at, y + at, 1.0),
    }
    p.regions.push(Region::new((x, y, BUTTON, BUTTON), 9.0, Action::TrayClick(i))
        .right(Action::TrayRight(i)).middle(Action::TrayMiddle(i)));
}

/// An app id as a name: `org.gnome.Nautilus` reads Nautilus.
fn app_name(appid: &str) -> String {
    let tail = appid.rsplit('.').next().unwrap_or(appid).replace(['-', '_'], " ");
    let mut chars = tail.chars();
    chars.next().map_or(String::new(), |first| first.to_uppercase().chain(chars).collect())
}

/// Workspaces as a column in one capsule: a dot when empty, icons when not.
fn tags(p: &mut Pen, c: &Ctx, top: f64, limit: f64) -> f64 {
    let Some(mon) = c.m.monitors.iter().find(|m| m.name == c.mon) else { return top };
    let pal = p.pal;
    let mut rows = Vec::new();
    let mut y = top + 4.0;
    for tag in &mon.tags {
        let windows: Vec<&wm::Client> = wm::clients_on(&c.m.clients, &mon.name, tag.index).collect();
        if windows.is_empty() && !tag.is_active && tag.index > PERSISTENT_TAGS {
            continue;
        }
        let n = windows.len().min(TAG_ICONS);
        let h = match (n, tag.is_active) {
            (0, false) => 22.0,
            (0, true) => BUTTON,
            (n, _) => 6.0 + n as f64 * 22.0,
        };
        if y + h + 4.0 > limit {
            break;
        }
        rows.push((tag, windows, y, h));
        y += h + 2.0;
    }
    let Some(last) = rows.last() else { return top };
    let end = last.2 + last.3 + 4.0;
    let x = (RAIL - BUTTON) / 2.0;
    p.fill((x - 2.0, top, BUTTON + 4.0, end - top), (BUTTON + 4.0) / 2.0, pal.fg, 0.07);
    for (tag, windows, y, h) in rows {
        let rect = (x, y, BUTTON, h);
        if tag.is_active {
            p.fill(rect, BUTTON / 2.0, pal.accent, 1.0);
        }
        let tone = match () {
            _ if tag.is_urgent => pal.error,
            _ if tag.is_active => pal.on_accent,
            _ if windows.is_empty() => pal.muted,
            _ => pal.fg,
        };
        p.regions.push(Region::new(rect, BUTTON / 2.0, Action::View { tag: tag.index, mon: c.mon.into() })
            .right(Action::MoveTo(tag.index)));
        if windows.is_empty() {
            let d = if tag.is_active { 8.0 } else { 5.0 };
            p.fill(((RAIL - d) / 2.0, y + (h - d) / 2.0, d, d), d / 2.0, tone, 1.0);
        }
        for (i, win) in windows.iter().take(TAG_ICONS).enumerate() {
            let iy = y + 5.0 + i as f64 * 22.0;
            let tone = if win.is_urgent { pal.error } else { tone };
            p.app(win.appid.as_deref().unwrap_or(""), ICON, (RAIL - ICON) / 2.0, iy, tone, 1.0);
            p.regions.push(Region::new((x + 2.0, iy - 2.0, BUTTON - 4.0, 22.0), 8.0, Action::Focus(win.id))
                .right(Action::MoveTo(tag.index)));
        }
    }
    end
}

/// Down the frame's left side: workspaces, the window in front, tray, time and status.
pub fn rail(p: &mut Pen, c: &Ctx, height: f64) {
    let (pal, cfg) = (p.pal, c.cfg);
    let x = (RAIL - BUTTON) / 2.0;
    let mut top = 0.0;
    if !cfg.tools.is_empty() {
        Button::new("search", false, Action::Cmd("open".into())).draw(p, x, top);
        top += BUTTON + 10.0;
    }

    // From the bottom up, so what's left in the middle is the title's.
    let mut end = height;
    if has(&cfg.tools, "power") {
        end -= BUTTON;
        Button::new("power_settings_new", false, Action::Open("power")).draw(p, x, end);
        end -= 8.0;
    }
    let state = state(c);
    let batt = c.m.battery.as_ref().filter(|_| has(&cfg.pill, "battery"));
    let group = state.len() as f64 * BUTTON + if batt.is_some() { 40.0 } else { 0.0 };
    if group > 0.0 {
        end -= group + 8.0;
        p.fill((x - 2.0, end, BUTTON + 4.0, group + 8.0), (BUTTON + 4.0) / 2.0, pal.fg, 0.07);
        let mut y = end + 4.0;
        for b in &state {
            b.draw(p, x, y);
            y += BUTTON;
        }
        if let Some(b) = batt {
            let tone = if b.pct <= 15.0 && !b.plugged { pal.error } else { pal.fg };
            p.icon(battery::symbol(b.pct, b.charging), b.charging, ICON, (RAIL - ICON) / 2.0, y + 3.0, tone, 1.0);
            let pct = format!("{:.0}", b.pct);
            let w = p.width(&pct, 10.5, Weight::Semibold);
            p.text(&pct, 10.5, Weight::Semibold, tone, (RAIL - w) / 2.0, y + 30.0, -1.0);
        }
        end -= 10.0;
    }
    if has(&cfg.pill, "clock") {
        let (hours, minutes) = clock::stacked(&cfg.clock);
        end -= 34.0;
        for (i, s) in [hours, minutes].iter().enumerate() {
            let w = p.width(s, 13.0, Weight::Bold);
            p.text(s, 13.0, Weight::Bold, pal.accent, (RAIL - w) / 2.0, end + 8.0 + i as f64 * 17.0, -1.0);
        }
        let mut open = Region::new((x, end - 2.0, BUTTON, 38.0), 10.0, Action::Cmd("sidebar".into()));
        open.right = Action::Pin;
        p.regions.push(open);
        end -= 8.0;
    }
    if has(&cfg.panel, "status") {
        for i in (0..c.m.tray.len().min(TRAY)).rev() {
            end -= BUTTON;
            tray(p, c, i, x, end);
        }
    }

    if has(&cfg.pill, "workspaces") {
        top = tags(p, c, top, end - 12.0) + 14.0;
        // The app rather than its title: terminals rewrite titles all day, and each would be a redraw.
        let front = c.m.clients.iter().find(|w| w.is_focused && !w.is_minimized && w.monitor == c.mon);
        if let Some(appid) = front.and_then(|w| w.appid.as_deref()).filter(|_| end - top > 130.0) {
            let name = app_name(appid);
            let room = (end - top - 60.0).min(220.0);
            let w = p.width(&name, 12.0, Weight::Medium).min(room);
            let y = (top + end - w - 26.0) / 2.0;
            p.app(appid, 16.0, (RAIL - 16.0) / 2.0, y, pal.muted, 1.0);
            let _ = p.cr.save();
            p.cr.translate(RAIL / 2.0, y + 26.0);
            p.cr.rotate(std::f64::consts::FRAC_PI_2);
            p.text(&name, 12.0, Weight::Medium, pal.muted, 0.0, 0.0, room);
            let _ = p.cr.restore();
        }
    }
}

/// The two ends of the bar. Its middle is the island.
pub fn bar(p: &mut Pen, c: &Ctx, width: f64) {
    let cfg = c.cfg;
    let y = (crate::style::BAR - BUTTON) / 2.0;
    let mut x = 8.0;
    if !cfg.tools.is_empty() {
        Button::new("search", false, Action::Cmd("open".into())).draw(p, x, y);
        x += BUTTON + 6.0;
    }
    if has(&cfg.pill, "workspaces") {
        workspaces::pill(p, c, x);
    }

    let mut end = width - 8.0 - BUTTON;
    if has(&cfg.tools, "power") {
        Button::new("power_settings_new", false, Action::Open("power")).draw(p, end, y);
        end -= BUTTON;
    }
    Button::new("dock_to_right", false, Action::Cmd("sidebar".into())).draw(p, end, y);
    for b in state(c).iter().rev() {
        end -= BUTTON;
        b.draw(p, end, y);
    }
    if c.m.battery.is_some() && has(&cfg.pill, "battery") {
        end -= 62.0;
        battery::pill(p, c, end + 6.0);
    }
    if has(&cfg.panel, "status") {
        end -= 6.0;
        for i in (0..c.m.tray.len().min(TRAY)).rev() {
            end -= BUTTON;
            tray(p, c, i, end, y);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn app_ids_read_as_names() {
        assert_eq!(super::app_name("org.gnome.Nautilus"), "Nautilus");
        assert_eq!(super::app_name("zen-browser"), "Zen browser");
        assert_eq!(super::app_name(""), "");
    }
}
