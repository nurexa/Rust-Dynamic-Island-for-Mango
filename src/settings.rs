//! ~/.config/island/config.toml is hand-edited and optional. ~/.local/state/island/state
//! is written by the island: where each pill sits, and pin and tuck.
use serde::Deserialize;
use smithay_client_toolkit::seat::keyboard::Keysym;
use std::{collections::HashMap, env, fs, path::PathBuf};

pub fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| "/tmp".into())
}

pub fn state_dir() -> PathBuf {
    env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/state")).join("island")
}

fn state_path() -> PathBuf {
    state_dir().join("state")
}

#[derive(Deserialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum IconStyle {
    /// One-color Nerd Font glyphs in the theme's colors.
    Glyph,
    /// The apps' own full-color icons.
    Color,
}

/// Shell commands behind the panel's controls. Empty means no action; an empty
/// `wallpaper_pick` also hides the wallpaper button.
#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Commands {
    pub wifi: String,
    pub wifi_alt: String,
    pub mixer: String,
    pub wallpaper_pick: String,
    pub wallpaper_next: String,
    pub wallpaper_toggle: String,
    pub lock: String,
    /// Prefix of the wallpaper settings command, e.g. `wall opt`. Empty hides the settings in the panel.
    pub wallpaper_opt: String,
}

impl Default for Commands {
    fn default() -> Self {
        Commands { wifi: "nm-connection-editor".into(), wifi_alt: String::new(), mixer: "pavucontrol".into(),
                   wallpaper_pick: String::new(), wallpaper_next: String::new(), wallpaper_toggle: String::new(),
                   lock: "loginctl lock-session".into(),
                   wallpaper_opt: String::new() }
    }
}

/// A module made of a shell command: its first line of output is what shows.
#[derive(Deserialize, Clone, Debug)]
pub struct Custom {
    pub name: String,
    pub exec: String,
    /// Seconds between runs. 0 runs it once.
    #[serde(default)]
    pub interval: u64,
    #[serde(default)]
    pub on_click: String,
}

/// A palette tab made of two shell commands. `list` prints one item per line, and
/// `run` receives the chosen line wherever it says `{}`.
#[derive(Deserialize, Clone, Debug)]
pub struct ScriptTool {
    pub name: String,
    #[serde(default = "default_tool_glyph")]
    pub glyph: String,
    pub hint: Option<String>,
    pub list: String,
    pub run: String,
}

fn default_tool_glyph() -> String {
    "\u{f0349}".into()
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Wallpaper {
    /// Folder to list. Empty hides the tool.
    pub dir: String,
    /// Optional thumbnail path, with `{file}` for the file's name.
    pub thumb: String,
    /// Runs on choosing one, with `{path}` as the file.
    pub set: String,
    /// Read by wall/wall, not by the island. Declared so the table documents itself.
    pub size: String,
    pub on_change: String,
}

#[derive(Deserialize, Clone, Debug)]
#[serde(default)]
pub struct NotifyTimes {
    pub low: u64,
    pub normal: u64,
}

impl Default for NotifyTimes {
    fn default() -> Self {
        NotifyTimes { low: 3, normal: 5 }
    }
}

/// What a palette key does.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum KeyAct { Up, Down, PageUp, PageDown, Accept, Close, NextTool, PrevTool, Clear, DeleteWord }

/// Palette keys, as lists of chords like `"ctrl+n"` or `"shift+tab"`. A list you set
/// replaces the default; an empty list turns the action off.
#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Keys {
    pub up: Vec<String>,
    pub down: Vec<String>,
    pub page_up: Vec<String>,
    pub page_down: Vec<String>,
    pub accept: Vec<String>,
    pub close: Vec<String>,
    pub next_tool: Vec<String>,
    pub prev_tool: Vec<String>,
    pub clear: Vec<String>,
    pub delete_word: Vec<String>,
}

impl Default for Keys {
    fn default() -> Self {
        let l = |l: &[&str]| l.iter().map(|s| s.to_string()).collect();
        Keys { up: l(&["up", "ctrl+p", "ctrl+k"]), down: l(&["down", "ctrl+n", "ctrl+j"]),
               page_up: l(&["pageup"]), page_down: l(&["pagedown"]), accept: l(&["enter", "kp_enter"]),
               close: l(&["escape"]), next_tool: l(&["tab"]), prev_tool: l(&["shift+tab"]),
               clear: l(&["ctrl+u"]), delete_word: l(&["ctrl+w", "ctrl+backspace"]) }
    }
}

/// A chord's key: a named key or one character. Err means the chord is unusable.
fn chord_key(name: &str) -> Option<Keysym> {
    Some(match name {
        "enter" | "return" => Keysym::Return,
        "kp_enter" => Keysym::KP_Enter,
        "escape" | "esc" => Keysym::Escape,
        "tab" => Keysym::Tab,
        "backspace" => Keysym::BackSpace,
        "space" => Keysym::space,
        "up" => Keysym::Up,
        "down" => Keysym::Down,
        "pageup" => Keysym::Page_Up,
        "pagedown" => Keysym::Page_Down,
        _ => {
            let mut c = name.chars();
            let (Some(ch), None) = (c.next(), c.next()) else { return None };
            Keysym::from_char(ch)
        }
    })
}

/// (ctrl, alt, shift, key) of a chord string.
fn parse_chord(chord: &str) -> Option<(bool, bool, bool, Keysym)> {
    let lower = chord.to_lowercase();
    let mut parts = lower.split('+').map(str::trim).collect::<Vec<_>>();
    let key = chord_key(parts.pop()?)?;
    let has = |m: &str| parts.contains(&m);
    parts.iter().all(|p| ["ctrl", "alt", "shift"].contains(p)).then(|| (has("ctrl"), has("alt"), has("shift"), key))
}

impl Keys {
    fn table(&self) -> [(KeyAct, &Vec<String>); 10] {
        use KeyAct::*;
        [(Up, &self.up), (Down, &self.down), (PageUp, &self.page_up), (PageDown, &self.page_down),
         (Accept, &self.accept), (Close, &self.close), (NextTool, &self.next_tool), (PrevTool, &self.prev_tool),
         (Clear, &self.clear), (DeleteWord, &self.delete_word)]
    }

    /// The action bound to this key press, if any. Modifiers must match exactly, and a
    /// letter matches either case. Shift+Tab arrives as its own key, so fold it back.
    pub fn action(&self, sym: Keysym, ctrl: bool, alt: bool, shift: bool) -> Option<KeyAct> {
        let (sym, shift) = if sym == Keysym::ISO_Left_Tab { (Keysym::Tab, true) } else { (sym, shift) };
        let fold = |k: Keysym| k.key_char().map_or(k, |c| Keysym::from_char(c.to_ascii_lowercase()));
        self.table().into_iter().find_map(|(act, chords)| {
            chords.iter().filter_map(|c| parse_chord(c)).any(|c| c == (ctrl, alt, shift, fold(sym)) ).then_some(act)
        })
    }

    /// Say which chords are unusable, once, when the config loads.
    fn warn(&self) {
        for (_, chords) in self.table() {
            for c in chords.iter().filter(|c| parse_chord(c).is_none()) {
                eprintln!("island: [keys] can't read {c:?}; try \"ctrl+n\", \"shift+tab\", \"escape\"");
            }
        }
    }
}

/// Look and feel switches.
#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Ui {
    /// Hovering the pill opens the panel. Off leaves it to a click or the pin key.
    pub hover_open: bool,
    pub hover_open_ms: u64,
    pub hover_close_ms: u64,
}

impl Default for Ui {
    fn default() -> Self {
        Ui { hover_open: true, hover_open_ms: 130, hover_close_ms: 280 }
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Config {
    pub font: String,
    pub icon_font: String,
    pub icon_style: IconStyle,
    pub clock: String,
    pub palette: String,
    /// Module order in the pill and the panel. See src/modules/mod.rs.
    pub pill: Vec<String>,
    pub panel: Vec<String>,
    /// App id -> glyph, over the built-in table.
    pub icons: HashMap<String, String>,
    pub commands: Commands,
    pub custom: Vec<Custom>,
    /// Palette tabs, in order. See src/tools/mod.rs.
    pub tools: Vec<String>,
    pub tool: Vec<ScriptTool>,
    /// Act as the notification daemon. Turn off to keep your own.
    pub notifications: bool,
    pub notify: NotifyTimes,
    pub wallpaper: Wallpaper,
    pub terminal: String,
    pub screenshot_dir: String,
    pub keys: Keys,
    pub ui: Ui,
}

impl Default for Config {
    fn default() -> Self {
        let list = |l: &[&str]| l.iter().map(|s| s.to_string()).collect();
        Config {
            font: "Adwaita Sans".into(),
            // Any Nerd Font has the icons, and this one is a sixth the size of Iosevka,
            // which cuts memory in half: HarfBuzz's tables scale with the font.
            icon_font: "JetBrainsMono Nerd Font Propo".into(),
            icon_style: IconStyle::Glyph,
            clock: "%I:%M %p".into(),
            palette: "~/.config/theme/generated/colors.toml".into(),
            pill: list(&["media", "workspaces", "network", "clock"]),
            panel: list(&["header", "workspaces", "media", "notifications", "stats", "audio", "status", "wallpaper"]),
            icons: HashMap::new(),
            commands: Commands::default(),
            custom: Vec::new(),
            tools: list(&["apps", "run", "windows", "clipboard", "screenshot", "wifi", "wallpaper", "power"]),
            tool: Vec::new(),
            notifications: true,
            notify: NotifyTimes::default(),
            wallpaper: Wallpaper::default(),
            terminal: "kitty".into(),
            screenshot_dir: "~/Pictures".into(),
            keys: Keys::default(),
            ui: Ui::default(),
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        env::var_os("XDG_CONFIG_HOME").map_or(home().join(".config"), PathBuf::from).join("island/config.toml")
    }

    pub fn parse(src: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(src)
    }

    /// A broken config must not take the bar down with it: complain, use defaults.
    pub fn load() -> Self {
        let Ok(src) = fs::read_to_string(Self::path()) else { return Config::default() };
        match Self::parse(&src) {
            Ok(c) => {
                c.keys.warn();
                c
            }
            Err(e) => {
                eprintln!("island: ignoring {}: {e}", Self::path().display());
                Config::default()
            }
        }
    }
}

#[derive(Default, Debug, PartialEq)]
pub struct State {
    /// Output name -> pill centre x, pill top y. Absent means docked top-centre.
    pub pos: HashMap<String, (f64, f64)>,
    pub pinned: bool,
    pub tucked: bool,
    pub dnd: bool,
}

impl State {
    pub fn parse(src: &str) -> Self {
        let mut s = State::default();
        for line in src.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            match f[..] {
                ["pos", name, x, y] => {
                    if let (Ok(x), Ok(y)) = (x.parse(), y.parse()) {
                        s.pos.insert(name.into(), (x, y));
                    }
                }
                ["pinned"] => s.pinned = true,
                ["tucked"] => s.tucked = true,
                ["dnd"] => s.dnd = true,
                _ => {}
            }
        }
        s
    }

    fn render(&self) -> String {
        let mut out = String::new();
        for (name, (x, y)) in &self.pos {
            out += &format!("pos {name} {x} {y}\n");
        }
        if self.pinned {
            out += "pinned\n";
        }
        if self.tucked {
            out += "tucked\n";
        }
        if self.dnd {
            out += "dnd\n";
        }
        out
    }

    pub fn load() -> Self {
        Self::parse(&fs::read_to_string(state_path()).unwrap_or_default())
    }

    /// Rename into place so a crash mid-write can't lose the old state.
    pub fn save(&self) {
        let path = state_path();
        let tmp = path.with_extension("tmp");
        let written = path.parent().map_or(Ok(()), fs::create_dir_all)
            .and_then(|_| fs::write(&tmp, self.render()))
            .and_then(|_| fs::rename(&tmp, &path));
        if let Err(e) = written {
            eprintln!("island: can't save {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_fills_gaps_and_rejects_nonsense() {
        let c = Config::parse("clock = \"%H:%M\"\npill = [\"clock\"]\n[icons]\nfoot = \"x\"\n[[custom]]\nname = \"w\"\nexec = \"date\"\n").unwrap();
        assert_eq!(c.clock, "%H:%M");
        assert_eq!(c.pill, ["clock"]);
        assert_eq!(c.panel.len(), Config::default().panel.len());
        assert_eq!(c.custom[0].interval, 0);
        assert!(Config::parse("icon_style = \"sparkly\"").is_err());
    }

    #[test]
    fn keys_rebind_disable_and_fold() {
        let k = |s| Config::parse(s).unwrap().keys;
        let d = Config::default().keys;
        assert_eq!(d.action(Keysym::n, true, false, false), Some(KeyAct::Down));
        assert_eq!(d.action(Keysym::N, true, false, false), Some(KeyAct::Down), "case folds");
        assert_eq!(d.action(Keysym::ISO_Left_Tab, false, false, true), Some(KeyAct::PrevTool));
        assert_eq!(d.action(Keysym::Escape, false, false, false), Some(KeyAct::Close));
        let c = k("[keys]\ndown = [\"alt+s\"]\nup = []\nclose = [\"nonsense+\"]");
        assert_eq!(c.action(Keysym::s, false, true, false), Some(KeyAct::Down));
        assert_eq!(c.action(Keysym::n, true, false, false), None, "list replaces the default");
        assert_eq!(c.action(Keysym::Up, false, false, false), None, "empty turns it off");
        assert_eq!(c.action(Keysym::Escape, false, false, false), None, "bad chord is ignored");
        assert!(!Config::parse("[ui]\nhover_open = false").unwrap().ui.hover_open);
    }

    #[test]
    fn state_round_trips_and_ignores_garbage() {
        let mut s = State { pinned: true, ..Default::default() };
        s.pos.insert("DP-2".into(), (640.5, 900.0));
        assert_eq!(State::parse(&s.render()), s);
        assert_eq!(State::parse("pos DP-2 x y\nnonsense\n\n"), State::default());
    }
}
