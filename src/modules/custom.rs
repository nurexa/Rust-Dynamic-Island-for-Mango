//! Modules made of a shell command, declared in config.toml as `[[custom]]` and
//! placed in a layout as `custom:<name>`. Each runs on a thread that sleeps
//! between runs, so a quiet one costs nothing.
use super::{Ctx, PAD, PANEL_W, PILL_H};
use crate::{
    Msg,
    pen::{Action, Pen, Region},
    settings::Custom,
};
use pango::Weight;
use std::{process::Command, thread, time::Duration};

pub fn spawn(list: &[Custom], tx: crate::Tx) {
    for module in list.iter().cloned() {
        let tx = tx.clone();
        thread::spawn(move || loop {
            let out = Command::new("sh").arg("-c").arg(&module.exec).output();
            let text = out.map(|o| String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("").to_owned());
            if tx.send(Msg::Custom(module.name.clone(), text.unwrap_or_default())).is_err() || module.interval == 0 {
                return;
            }
            thread::sleep(Duration::from_secs(module.interval));
        });
    }
}

fn click(c: &Ctx, name: &str) -> Action {
    let module = c.cfg.custom.iter().find(|m| m.name == name);
    module.filter(|m| !m.on_click.is_empty()).map_or(Action::None, |m| Action::Sh(m.on_click.clone()))
}

pub fn pill(p: &mut Pen, c: &Ctx, name: &str, x: f64) -> f64 {
    let Some(text) = c.m.custom.get(name).filter(|t| !t.is_empty()) else { return x };
    let cy = PILL_H / 2.0;
    let w = p.text(text, 12.5, Weight::Semibold, p.pal.fg, x, cy, 180.0);
    p.regions.push(Region::new((x - 4.0, cy - 12.0, w + 8.0, 24.0), 8.0, click(c, name)));
    x + w
}

pub fn panel(p: &mut Pen, c: &Ctx, name: &str, y: f64) -> f64 {
    let Some(text) = c.m.custom.get(name).filter(|t| !t.is_empty()) else { return y };
    let h = 28.0;
    let w = p.text(text, 12.5, Weight::Medium, p.pal.fg, PAD, y + h / 2.0, PANEL_W - 2.0 * PAD);
    p.regions.push(Region::new((PAD - 6.0, y, w + 12.0, h), 10.0, click(c, name)));
    y + h + 4.0
}
