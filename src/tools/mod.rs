//! Palette tools: everything that used to be rofi, wlogout and friends. A tool is
//! a function that lists items, one file each, with a line in `TOOLS`. Lists are
//! built on a worker thread when a tool opens and dropped when it closes, so a tool
//! costs nothing until you use it. Choose them with `tools` in config.toml, or leave
//! them out of the build entirely with Cargo features.
use crate::{Msg, Tx, settings::Config, wm};
use std::{
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
    thread,
};

#[cfg(feature = "apps")]
pub mod apps;
#[cfg(feature = "clipboard")]
mod clipboard;
#[cfg(feature = "power")]
mod power;
#[cfg(feature = "run")]
mod run;
#[cfg(feature = "screenshot")]
pub mod screenshot;
#[cfg(feature = "wallpaper")]
mod wallpaper;
#[cfg(feature = "wifi")]
mod wifi;
#[cfg(feature = "windows")]
mod windows;

#[derive(Clone, Debug, PartialEq)]
pub enum Icon {
    None,
    /// One-color Nerd Font glyph.
    Glyph(String),
    /// Name from an icon theme, or a path, as in a `.desktop` file's Icon=.
    Named(String),
    /// An image file, such as a wallpaper thumbnail.
    Image(PathBuf),
}

/// What happens when an item is chosen.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Sh(String),
    /// A desktop app: `id` feeds launch counts, so favourites rise to the top.
    Launch { id: String, cmd: String },
    Focus(u64),
    Quit,
    /// Asks for a second Enter first, showing this text on the row.
    Confirm(String, Box<Act>),
    /// Shows a text field, then runs the action with what was typed.
    Ask { label: String, password: bool, then: Then },
    /// Hands the text back to whoever called `island dmenu`.
    Reply(String),
    /// Run a tool's own code on a worker thread.
    Task(Task),
    /// Expands into a nested list; Esc or `Back` returns to the one it came from.
    Open(Vec<Item>),
    Back,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Then {
    #[cfg(feature = "wifi")]
    WifiPassword(String),
    /// Exists so this enum isn't empty when every tool is compiled out.
    Nothing,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Task {
    #[cfg(feature = "clipboard")]
    Copy(String),
    #[cfg(feature = "screenshot")]
    Shot(screenshot::Kind),
    #[cfg(feature = "wifi")]
    Wifi(wifi::Pick),
    /// Exists so this enum isn't empty when every tool is compiled out.
    Nothing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub title: String,
    pub sub: String,
    pub icon: Icon,
    pub act: Act,
    /// Added to the match score: how launch counts lift favourites.
    pub boost: i32,
}

impl Item {
    pub fn new(title: impl Into<String>, icon: Icon, act: Act) -> Self {
        Item { title: title.into(), sub: String::new(), icon, act, boost: 0 }
    }

    pub fn sub(mut self, sub: impl Into<String>) -> Self {
        self.sub = sub.into();
        self
    }
}

/// What a tool's list function can read. Owned, because it runs on a worker.
#[cfg_attr(not(feature = "windows"), allow(dead_code))]
pub struct Ctx {
    pub cfg: Config,
    pub clients: Vec<wm::Client>,
}

pub struct Tool {
    pub name: &'static str,
    pub glyph: &'static str,
    pub hint: &'static str,
    pub list: fn(&Ctx) -> Vec<Item>,
    /// Lets Enter act on what was typed when nothing in the list matches.
    pub typed: Option<fn(&str) -> Act>,
}

pub const TOOLS: &[Tool] = &[
    #[cfg(feature = "apps")]
    Tool { name: "apps", glyph: "\u{f003b}", hint: "Search apps", list: apps::list, typed: None },
    #[cfg(feature = "run")]
    Tool { name: "run", glyph: "\u{f018d}", hint: "Run a command", list: run::list, typed: Some(run::typed) },
    #[cfg(feature = "windows")]
    Tool { name: "windows", glyph: "\u{f05b2}", hint: "Switch to a window", list: windows::list, typed: None },
    #[cfg(feature = "clipboard")]
    Tool { name: "clipboard", glyph: "\u{f0192}", hint: "Search clipboard history", list: clipboard::list, typed: None },
    #[cfg(feature = "screenshot")]
    Tool { name: "screenshot", glyph: "\u{f0100}", hint: "Capture the screen", list: screenshot::list, typed: None },
    #[cfg(feature = "wifi")]
    Tool { name: "wifi", glyph: "\u{f0928}", hint: "Choose a network", list: wifi::list, typed: None },
    #[cfg(feature = "wallpaper")]
    Tool { name: "wallpaper", glyph: "\u{f0e09}", hint: "Choose a wallpaper", list: wallpaper::list, typed: None },
    #[cfg(feature = "power")]
    Tool { name: "power", glyph: "\u{f0425}", hint: "Lock, log out or shut down", list: power::list, typed: None },
];

/// A tool in the palette: built in, or a pair of shell commands from `[[tool]]`.
#[derive(Clone, Debug)]
pub enum Entry {
    Builtin(usize),
    Script(crate::settings::ScriptTool),
    /// `island dmenu`: its items came in on the socket.
    Dmenu(String),
}

impl Entry {
    pub fn name(&self) -> &str {
        match self {
            Entry::Builtin(i) => TOOLS[*i].name,
            Entry::Script(s) => &s.name,
            Entry::Dmenu(_) => "dmenu",
        }
    }

    pub fn glyph(&self) -> &str {
        match self {
            Entry::Builtin(i) => TOOLS[*i].glyph,
            Entry::Script(s) => &s.glyph,
            Entry::Dmenu(_) => "\u{f0349}",
        }
    }

    pub fn hint(&self) -> String {
        match self {
            Entry::Builtin(i) => TOOLS[*i].hint.to_owned(),
            Entry::Script(s) => s.hint.clone().unwrap_or_else(|| format!("Search {}", s.name)),
            Entry::Dmenu(prompt) => prompt.clone(),
        }
    }

    pub fn typed(&self) -> Option<fn(&str) -> Act> {
        match self {
            Entry::Builtin(i) => TOOLS[*i].typed,
            _ => None,
        }
    }
}

/// The palette's tabs, in the order config.toml gives. Tools compiled out or
/// missing their dependencies simply don't appear.
pub fn enabled(cfg: &Config) -> Vec<Entry> {
    cfg.tools.iter().filter_map(|name| {
        let builtin = TOOLS.iter().position(|t| t.name == name).map(Entry::Builtin);
        builtin.or_else(|| cfg.tool.iter().find(|t| t.name == *name).cloned().map(Entry::Script))
    }).filter(|e| usable(e, cfg)).collect()
}

fn usable(entry: &Entry, cfg: &Config) -> bool {
    match entry.name() {
        "clipboard" => on_path("cliphist") && on_path("wl-copy"),
        "screenshot" => on_path("grim"),
        "wifi" => on_path("nmcli"),
        "wallpaper" => !cfg.wallpaper.dir.is_empty(),
        _ => true,
    }
}

pub fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}

/// Single-quote for `sh`, so filenames and script output can't become commands.
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Lines from a `[[tool]]` script, each running `run` with `{}` as that line.
fn script_items(tool: &crate::settings::ScriptTool) -> Vec<Item> {
    let Ok(out) = Command::new("sh").arg("-c").arg(&tool.list).output() else { return Vec::new() };
    String::from_utf8_lossy(&out.stdout).lines().filter(|l| !l.is_empty()).map(|line| {
        Item::new(line, Icon::None, Act::Sh(tool.run.replace("{}", &quote(line))))
    }).collect()
}

/// Build a tool's list on a worker and post it back, tagged so a slow list from a
/// tool you've since left can't replace the one you're looking at.
pub fn load(entry: &Entry, generation: u64, ctx: Ctx, tx: Tx) {
    let entry = entry.clone();
    thread::spawn(move || {
        let items = match &entry {
            Entry::Builtin(i) => (TOOLS[*i].list)(&ctx),
            Entry::Script(s) => script_items(s),
            Entry::Dmenu(_) => return,
        };
        let _ = tx.send(Msg::Items(generation, items));
    });
}

/// In its own session, so apps outlive the island and ignore its signals.
pub fn spawn_detached(cmd: &str) {
    let child = Command::new("sh").arg("-c").arg(cmd).stdin(Stdio::null())
        .stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn();
    if let Ok(mut child) = child {
        thread::spawn(move || child.wait());
    }
}

/// Hand a note of our own to whichever notification daemon is running instead.
pub fn spawn_note(note: &crate::notify::Note) {
    let child = Command::new("notify-send").arg("-a").arg(&note.app).arg(&note.title).arg(&note.body)
        .stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    if let Ok(mut child) = child {
        thread::spawn(move || child.wait());
    }
}

/// Work chosen from a list. Always off the UI thread: these wait on other programs.
pub fn run_task(task: Task, ctx: Ctx, tx: Tx) {
    thread::spawn(move || match task {
        #[cfg(feature = "clipboard")]
        Task::Copy(line) => clipboard::copy(&line),
        #[cfg(feature = "screenshot")]
        Task::Shot(kind) => screenshot::take(kind, &ctx, &tx),
        #[cfg(feature = "wifi")]
        Task::Wifi(pick) => wifi::connect(pick, &tx),
        Task::Nothing => {}
    });
    #[cfg(not(any(feature = "screenshot", feature = "wifi")))]
    let _ = (ctx, tx);
}

pub fn run_then(then: Then, text: String, tx: Tx) {
    match then {
        #[cfg(feature = "wifi")]
        Then::WifiPassword(ssid) => {
            thread::spawn(move || wifi::connect_with_password(&ssid, &text, &tx));
        }
        Then::Nothing => drop((text, tx)),
    }
}

/// Screenshot kind from `island shot <kind>`.
#[cfg(feature = "screenshot")]
pub fn shot_task(kind: &str) -> Option<Task> {
    screenshot::Kind::parse(kind).map(Task::Shot)
}

#[cfg(not(feature = "screenshot"))]
pub fn shot_task(_: &str) -> Option<Task> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_keeps_hostile_filenames_out_of_the_shell() {
        let name = "a'; rm -rf ~; '.png";
        let out = Command::new("sh").arg("-c").arg(format!("printf %s {}", quote(name))).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), name);
    }

    #[test]
    fn config_decides_which_tools_appear_and_in_what_order() {
        let mut cfg = Config::default();
        cfg.tools = vec!["power".into(), "nonsense".into(), "run".into()];
        let names: Vec<String> = enabled(&cfg).iter().map(|e| e.name().to_owned()).collect();
        assert_eq!(names, ["power", "run"]);
    }
}
