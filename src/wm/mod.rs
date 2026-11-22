//! The window manager, behind one small trait. Everything above this line deals
//! in monitors, tags and windows, so supporting another compositor means one new
//! file here and one new line in `detect`. MODDING.md walks through it.
use crate::Tx;
use serde::Deserialize;

mod hyprland;
mod mango;

#[derive(Deserialize, Clone, PartialEq, Debug)]
pub struct Tag {
    pub index: u32,
    pub is_active: bool,
    pub is_urgent: bool,
}

#[derive(Deserialize, Clone, PartialEq, Debug)]
pub struct Monitor {
    pub name: String,
    /// True on the monitor mango has selected, not merely enabled.
    pub active: bool,
    pub layout_symbol: String,
    pub tags: Vec<Tag>,
}

#[derive(Deserialize, Clone, PartialEq, Debug)]
pub struct Client {
    pub id: u64,
    /// The process behind the window, when the compositor says.
    #[serde(default)]
    pub pid: u32,
    pub title: Option<String>,
    pub appid: Option<String>,
    pub monitor: String,
    #[serde(default)]
    pub tags: Vec<u32>,
    pub is_focused: bool,
    pub is_minimized: bool,
    pub is_urgent: bool,
    #[serde(default)]
    pub is_fullscreen: bool,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    #[serde(default)]
    pub width: i32,
    #[serde(default)]
    pub height: i32,
}

/// What the island needs from a compositor. `watch` must send `Msg::Monitors` and
/// `Msg::Clients` whenever those change. Actions must not block the UI thread.
pub trait Backend {
    fn watch(&self, tx: Tx);
    /// Show a workspace on a monitor, focusing that monitor.
    fn view(&self, tag: u32, monitor: &str);
    /// Move the focused window to a workspace, staying where you are.
    fn send_to(&self, tag: u32);
    fn focus(&self, window: u64);
    /// Step to the next or previous occupied workspace.
    fn cycle(&self, forward: bool);
    /// End the session.
    fn quit(&self);
}

/// No compositor integration: the workspaces module stays empty and every other
/// module works, on anything with wlr-layer-shell.
struct Bare;

impl Backend for Bare {
    fn watch(&self, _: Tx) {}
    fn view(&self, _: u32, _: &str) {}
    fn send_to(&self, _: u32) {}
    fn focus(&self, _: u64) {}
    fn cycle(&self, _: bool) {}
    fn quit(&self) {}
}

pub fn detect() -> Box<dyn Backend> {
    if std::env::var_os("MANGO_INSTANCE_SIGNATURE").is_some() {
        return Box::new(mango::Mango);
    }
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        return Box::new(hyprland::Hyprland);
    }
    eprintln!("island: no supported compositor found, so workspaces stay empty (see MODDING.md)");
    Box::new(Bare)
}

pub fn same_but_titles(a: &[Client], b: &[Client]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| {
        (a.id, &a.appid, &a.monitor, &a.tags, a.is_focused, a.is_minimized, a.is_urgent, a.is_fullscreen)
            == (b.id, &b.appid, &b.monitor, &b.tags, b.is_focused, b.is_minimized, b.is_urgent, b.is_fullscreen)
    })
}

/// A fullscreen window is on show on this monitor, over the island.
pub fn covered(clients: &[Client], monitors: &[Monitor], name: &str) -> bool {
    let Some(mon) = monitors.iter().find(|m| m.name == name) else { return false };
    mon.tags.iter().filter(|t| t.is_active).any(|t| clients_on(clients, name, t.index).any(|c| c.is_fullscreen))
}

/// Windows on one tag of one monitor, in mango's order. Minimized windows stay
/// out, matching what the Waybar pills showed.
pub fn clients_on<'a>(clients: &'a [Client], mon: &'a str, tag: u32) -> impl Iterator<Item = &'a Client> {
    clients.iter().filter(move |c| !c.is_minimized && c.monitor == mon && c.tags.contains(&tag))
}
