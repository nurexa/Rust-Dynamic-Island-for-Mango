//! ~/.config/island/config.toml is hand-edited and optional. ~/.local/state/island/state
//! is written by the island: where each pill sits, and pin and tuck.
use serde::Deserialize;
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
        Self::parse(&src).unwrap_or_else(|e| {
            eprintln!("island: ignoring {}: {e}", Self::path().display());
            Config::default()
        })
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
    fn state_round_trips_and_ignores_garbage() {
        let mut s = State { pinned: true, ..Default::default() };
        s.pos.insert("DP-2".into(), (640.5, 900.0));
        assert_eq!(State::parse(&s.render()), s);
        assert_eq!(State::parse("pos DP-2 x y\nnonsense\n\n"), State::default());
    }
}
