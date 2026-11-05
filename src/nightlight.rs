//! Night light: the island warms and dims the screen itself, through
//! wlr-gamma-control, so there's no wlsunset or gammastep to run. While it's on we
//! hold one gamma control per output; off destroys them and the compositor puts
//! the original tables back. A schedule rides the clock's minute tick, so it costs
//! no wakeups of its own.
use crate::App;
use serde::Deserialize;
use smithay_client_toolkit::{
    dispatch2::Dispatch2,
    reexports::{
        client::{Connection, QueueHandle, globals::GlobalList, protocol::wl_output::WlOutput},
        protocols_wlr::gamma_control::v1::client::{
            zwlr_gamma_control_manager_v1::ZwlrGammaControlManagerV1,
            zwlr_gamma_control_v1::{Event, ZwlrGammaControlV1},
        },
    },
};
use std::{fs::File, os::fd::{AsFd, FromRawFd}, os::unix::fs::FileExt};

pub const MIN_K: u32 = 2500;
/// Daylight: the screen as it is.
pub const MAX_K: u32 = 6500;

/// `[nightlight]` in config.toml.
#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Settings {
    /// "20:00-07:00" turns it on and off by itself. Empty is manual only.
    pub schedule: String,
    /// Kelvin until the panel slider picks another; the slider's choice is remembered.
    pub warmth: u32,
    /// Brightness while on, 0-1.
    pub dim: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { schedule: String::new(), warmth: 3400, dim: 0.85 }
    }
}

struct Ctl {
    output: WlOutput,
    gamma: ZwlrGammaControlV1,
    /// Entries per channel; 0 until the compositor says.
    size: u32,
}

pub struct Night {
    manager: Option<ZwlrGammaControlManagerV1>,
    ctls: Vec<Ctl>,
    /// Outputs that refused, so a slider drag doesn't ask them again on every step.
    refused: Vec<WlOutput>,
    worked: bool,
    warned: bool,
    pub on: bool,
    pub warmth: u32,
    dim: f64,
    schedule: Option<(u32, u32)>,
    /// Whether the clock was inside the schedule at the last minute tick. A manual
    /// toggle leaves it alone, so it holds until the schedule's next edge.
    inside: Option<bool>,
}

/// Minutes since midnight of "HH:MM-HH:MM".
fn parse_schedule(s: &str) -> Option<(u32, u32)> {
    let min = |t: &str| {
        let (h, m) = t.trim().split_once(':')?;
        let (h, m) = (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?);
        (h < 24 && m < 60).then_some(h * 60 + m)
    };
    let (a, b) = s.split_once('-')?;
    Some((min(a)?, min(b)?))
}

fn inside((start, end): (u32, u32), now: u32) -> bool {
    if start <= end { (start..end).contains(&now) } else { now >= start || now < end }
}

fn minute_of_day() -> u32 {
    pango::glib::DateTime::now_local().map_or(0, |d| d.hour() as u32 * 60 + d.minute() as u32)
}

/// Tanner Helland's fit of blackbody color, as 0-1 per channel.
fn blackbody(k: f64) -> [f64; 3] {
    let t = k / 100.0;
    let r = if t <= 66.0 { 255.0 } else { 329.698727446 * (t - 60.0).powf(-0.1332047592) };
    let g = if t <= 66.0 { 99.4708025861 * t.ln() - 161.1195681661 } else { 288.1221695283 * (t - 60.0).powf(-0.0755148492) };
    let b = if t >= 66.0 { 255.0 } else if t <= 19.0 { 0.0 } else { 138.5177312231 * (t - 10.0).ln() - 305.0447927307 };
    [r, g, b].map(|c| (c / 255.0).clamp(0.0, 1.0))
}

/// Channel multipliers for `k`, relative to 6500K so daylight changes nothing.
pub fn white(k: u32) -> [f64; 3] {
    let (c, d) = (blackbody(k as f64), blackbody(MAX_K as f64));
    [0, 1, 2].map(|i| (c[i] / d[i]).min(1.0))
}

/// Red, green and blue tables back to back, the layout set_gamma wants.
pub fn ramp(size: u32, k: u32, dim: f64) -> Vec<u16> {
    let last = size.saturating_sub(1).max(1) as f64;
    white(k).iter().flat_map(|m| (0..size).map(move |i| (i as f64 / last * m * dim * 65535.0).round() as u16)).collect()
}

fn upload(gamma: &ZwlrGammaControlV1, table: &[u16]) -> std::io::Result<()> {
    let fd = unsafe { libc::memfd_create(c"island-gamma".as_ptr(), libc::MFD_CLOEXEC) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let bytes: Vec<u8> = table.iter().flat_map(|v| v.to_ne_bytes()).collect();
    // At offset 0 without moving the file position: some compositors read(), not pread().
    file.write_all_at(&bytes, 0)?;
    gamma.set_gamma(file.as_fd());
    Ok(())
}

impl Night {
    pub fn new(globals: &GlobalList, qh: &QueueHandle<App>, cfg: &Settings, on: bool, warmth: Option<u32>) -> Self {
        let manager = globals.bind(qh, 1..=1, Gamma).ok();
        let mut n = Night { manager, ctls: Vec::new(), refused: Vec::new(), worked: false, warned: false,
                            on, warmth: 0, dim: 1.0, schedule: None, inside: None };
        n.configure(cfg);
        n.warmth = warmth.unwrap_or(cfg.warmth).clamp(MIN_K, MAX_K);
        // A schedule decides the state at start; the saved one is for manual use.
        if let Some(s) = n.schedule {
            n.inside = Some(inside(s, minute_of_day()));
            n.on = n.inside == Some(true);
        }
        n
    }

    /// Hidden when the compositor can't, or when every output we asked refused.
    pub fn usable(&self) -> bool {
        self.manager.is_some() && (self.worked || self.refused.is_empty())
    }

    /// Picks up a changed config. A new schedule takes effect at the next minute.
    pub fn configure(&mut self, cfg: &Settings) {
        // Floor so a typo can't black the screen out.
        self.dim = cfg.dim.clamp(0.2, 1.0);
        let schedule = parse_schedule(&cfg.schedule);
        if schedule.is_none() && !cfg.schedule.trim().is_empty() {
            eprintln!("island: [nightlight] schedule {:?} isn't \"HH:MM-HH:MM\"; manual only", cfg.schedule);
        }
        if schedule != self.schedule {
            self.schedule = schedule;
            self.inside = None;
        }
        // Retry outputs that refused: whoever held their gamma may be gone.
        self.refused.clear();
    }

    /// The new state when the schedule crossed an edge since the last minute.
    pub fn edge(&mut self) -> Option<bool> {
        let now = Some(inside(self.schedule?, minute_of_day()));
        (now != std::mem::replace(&mut self.inside, now)).then_some(now == Some(true))
    }

    /// Bring every output in line with `on` and `warmth`.
    pub fn apply(&mut self, outputs: &[WlOutput], qh: &QueueHandle<App>) {
        let Some(manager) = self.manager.as_ref().filter(|_| self.on) else {
            self.ctls.drain(..).for_each(|c| c.gamma.destroy());
            return;
        };
        for o in outputs {
            if !self.ctls.iter().any(|c| c.output == *o) && !self.refused.contains(o) {
                let gamma = manager.get_gamma_control(o, qh, Gamma);
                self.ctls.push(Ctl { output: o.clone(), gamma, size: 0 });
            }
        }
        for c in self.ctls.iter().filter(|c| c.size > 0) {
            self.write(c);
        }
    }

    fn write(&self, c: &Ctl) {
        if let Err(e) = upload(&c.gamma, &ramp(c.size, self.warmth, self.dim)) {
            eprintln!("island: night light: {e}");
        }
    }

    pub fn drop_output(&mut self, output: &WlOutput) {
        self.refused.retain(|o| o != output);
        self.ctls.retain(|c| c.output != *output || {
            c.gamma.destroy();
            false
        });
    }

    fn sized(&mut self, gamma: &ZwlrGammaControlV1, size: u32) {
        let Some(i) = self.ctls.iter().position(|c| c.gamma == *gamma) else { return };
        self.ctls[i].size = size;
        self.worked = true;
        self.write(&self.ctls[i]);
    }

    /// Another client holds the gamma, or the output has none.
    fn failed(&mut self, gamma: &ZwlrGammaControlV1) {
        let Some(i) = self.ctls.iter().position(|c| c.gamma == *gamma) else { return };
        let c = self.ctls.remove(i);
        c.gamma.destroy();
        self.refused.push(c.output);
        if !std::mem::replace(&mut self.warned, true) {
            eprintln!("island: night light refused (wlsunset or gammastep running, or no gamma on this output)");
        }
    }
}

impl App {
    pub(crate) fn night_apply(&mut self) {
        let outputs: Vec<WlOutput> = self.outputs.outputs().collect();
        self.model.night.apply(&outputs, &self.qh);
    }

    pub(crate) fn night_set(&mut self, on: bool) {
        self.model.night.on = on;
        self.state.night = on;
        self.save();
        self.night_apply();
        self.all(false, true);
    }

    /// The panel slider: right is warmer. Dragging it while off turns it on, so you see it.
    pub(crate) fn night_warmth(&mut self, frac: f64) {
        let k = (MAX_K as f64 - frac * (MAX_K - MIN_K) as f64) / 100.0;
        let k = k.round() as u32 * 100;
        // A drag fires per pixel; only act on a new 100K step.
        if k == self.model.night.warmth && self.model.night.on {
            return;
        }
        self.model.night.warmth = k;
        self.state.warmth = Some(k);
        self.night_set(true);
    }

    /// On the clock's minute tick.
    pub(crate) fn night_minute(&mut self) {
        if let Some(on) = self.model.night.edge() {
            self.night_set(on);
        }
    }

    /// `island nightlight [on|off|toggle]`
    pub(crate) fn night_command(&mut self, arg: &str) {
        match arg {
            "on" => self.night_set(true),
            "off" => self.night_set(false),
            "" | "toggle" => self.night_set(!self.model.night.on),
            _ => eprintln!("island: nightlight takes on, off or toggle"),
        }
    }
}

/// User data for the gamma objects; routes their events to `Night`.
pub struct Gamma;

impl Dispatch2<ZwlrGammaControlManagerV1, App> for Gamma {
    fn event(&self, _: &mut App, _: &ZwlrGammaControlManagerV1, _: <ZwlrGammaControlManagerV1 as smithay_client_toolkit::reexports::client::Proxy>::Event,
             _: &Connection, _: &QueueHandle<App>) {}
}

impl Dispatch2<ZwlrGammaControlV1, App> for Gamma {
    fn event(&self, app: &mut App, gamma: &ZwlrGammaControlV1, event: Event, _: &Connection, _: &QueueHandle<App>) {
        match event {
            Event::GammaSize { size } => app.model.night.sized(gamma, size),
            Event::Failed => {
                let was = app.model.night.usable();
                app.model.night.failed(gamma);
                if was != app.model.night.usable() {
                    app.all(false, true);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daylight_is_neutral_and_night_is_amber_and_dim() {
        assert_eq!(white(MAX_K), [1.0, 1.0, 1.0]);
        let [r, g, b] = white(3400);
        assert!(r == 1.0 && g < r && b < g && b > 0.3, "{r} {g} {b}");
        let day = ramp(256, MAX_K, 1.0);
        assert_eq!((day.len(), day[0], day[255], day[511], day[767]), (768, 0, 65535, 65535, 65535));
        let night = ramp(256, 3400, 0.85);
        assert_eq!(night[255], (0.85 * 65535.0f64).round() as u16);
        assert!(night[511] < night[255] && night[767] < night[511]);
        assert_eq!(parse_schedule("20:00-07:30"), Some((1200, 450)));
        assert_eq!(parse_schedule("25:00-07:00"), None);
        assert!(inside((1200, 420), 1380) && inside((1200, 420), 60) && !inside((1200, 420), 720));
        assert!(inside((60, 120), 90) && !inside((60, 120), 120));
    }
}
