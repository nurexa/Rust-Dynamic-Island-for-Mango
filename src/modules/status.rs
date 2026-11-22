//! Bottom row of the panel: network, battery and the tray.
use super::{Ctx, PAD, PILL_H};
use crate::{
    net,
    pen::{Action, Pen, Region},
};
use pango::Weight;

pub(super) fn net_symbol(state: &net::State) -> &'static str {
    match state {
        net::State::Wifi { strength, .. } => match strength {
            0..=19 => "signal_wifi_0_bar",
            20..=39 => "network_wifi_1_bar",
            40..=59 => "network_wifi_2_bar",
            60..=79 => "network_wifi_3_bar",
            _ => "signal_wifi_4_bar",
        },
        net::State::Wired { .. } => "lan",
        net::State::Other { .. } => "swap_vert",
        net::State::Offline | net::State::Unknown => "wifi_off",
    }
}

/// A quiet warning on the pill, only when the connection is down.
pub fn offline_pill(p: &mut Pen, c: &Ctx, x: f64) -> f64 {
    if c.m.net != net::State::Offline {
        return x;
    }
    p.symbol("wifi_off", 14.0, x, PILL_H / 2.0 - 7.0, p.pal.muted);
    x + 14.0
}

pub fn panel(p: &mut Pen, c: &Ctx, y: f64) -> f64 {
    let (pal, m, cmd) = (p.pal, c.m, &c.cfg.commands);
    let h = 30.0;
    let cy = y + h / 2.0;
    let label = match &m.net {
        net::State::Wifi { ssid, .. } => ssid.clone(),
        net::State::Wired { name } | net::State::Other { name } => name.clone(),
        net::State::Offline => "Disconnected".into(),
        net::State::Unknown => "Network".into(),
    };
    p.symbol(net_symbol(&m.net), 16.0, PAD + 1.0, cy - 8.0, pal.fg);
    let w = p.text(&label, 12.5, Weight::Medium, pal.fg, PAD + 30.0, cy, 168.0);
    let wifi = if c.cfg.tools.iter().any(|t| t == "wifi") { Action::Open("wifi") } else { Action::Sh(cmd.wifi.clone()) };
    p.regions.push(Region::new((PAD - 6.0, y, 40.0 + w, h), 10.0, wifi)
        .right(Action::Sh(cmd.wifi_alt.clone())));
    // UPower's view when there is one: it adds the time left.
    let upower = m.battery.as_ref();
    if let Some((pct, charging)) = upower.map(|b| (b.pct, b.charging)).or(m.sys.battery) {
        let x = PAD + 30.0 + w + 16.0;
        let symbol = super::battery::symbol(pct, charging);
        p.symbol(symbol, 16.0, x, cy - 8.0, pal.fg);
        let w = p.text(&format!("{pct:.0}%"), 12.5, Weight::Medium, pal.fg, x + 22.0, cy, -1.0);
        if let Some(t) = upower.and_then(|b| b.time()) {
            p.text(&t, 12.0, Weight::Medium, pal.muted, x + 22.0 + w + 8.0, cy, -1.0);
        }
    }
    let mut x = p.w - PAD - 30.0;
    for (i, (item, icon)) in m.tray.iter().enumerate().rev() {
        let r = (x, y, 30.0, h);
        match icon {
            Some((img, false)) => p.image(img, x + 6.0, cy - 9.0, 18.0, 1.0),
            Some((mask, true)) => {
                let s = mask.width() as f64 / 18.0;
                mask.set_device_scale(s, s);
                p.set(pal.fg, 1.0);
                let _ = p.cr.mask_surface(mask, x + 6.0, cy - 9.0);
            }
            None => p.letter(&item.title, 18.0, x + 6.0, cy - 9.0, 1.0),
        }
        p.regions.push(Region::new(r, 10.0, Action::TrayClick(i))
            .right(Action::TrayRight(i)).middle(Action::TrayMiddle(i)).tip(item.title.clone()));
        x -= 32.0;
    }
    y + h + 12.0
}
