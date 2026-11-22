//! ~/.config/island/config.toml is hand-edited and optional. ~/.local/state/island/state
//! is written by the island: where each pill sits, and pin and tuck.
use crate::style::Style;
use serde::Deserialize;
use smithay_client_toolkit::seat::keyboard::Keysym;
use std::{collections::{BTreeMap, HashMap}, env, fs, path::PathBuf};

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
    /// Prints the brightness as 0-100. Empty reads /sys/class/backlight; no backlight hides the slider.
    pub brightness_get: String,
    /// Sets it, with `{}` as the percentage. Empty uses `brightnessctl`.
    pub brightness_set: String,
}

impl Default for Commands {
    fn default() -> Self {
        Commands { wifi: "nm-connection-editor".into(), wifi_alt: String::new(), mixer: "pavucontrol".into(),
                   wallpaper_pick: String::new(), wallpaper_next: String::new(), wallpaper_toggle: String::new(),
                   lock: "loginctl lock-session".into(),
                   wallpaper_opt: String::new(), brightness_get: String::new(), brightness_set: String::new() }
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
    "search".into()
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

/// Minutes for each pomodoro phase, and a long break after `rounds` focus rounds.
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(default)]
pub struct Pomodoro {
    pub focus: u64,
    pub short: u64,
    pub long: u64,
    pub rounds: u32,
    /// Start the next phase on its own; off waits for a click.
    pub auto: bool,
}

impl Default for Pomodoro {
    fn default() -> Self {
        Pomodoro { focus: 25, short: 5, long: 15, rounds: 4, auto: true }
    }
}

/// Claude Code on the island (needs its hooks: ./install.sh --manual, or `island claude-setup`).
#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Claude {
    /// Permission prompts become Allow/Deny cards when you're not looking at that terminal.
    pub approve: bool,
    /// A note when a session finishes or needs you and its terminal isn't focused.
    pub notify: bool,
}

impl Default for Claude {
    fn default() -> Self {
        Claude { approve: true, notify: true }
    }
}

/// The calendar, wherever it's placed.
#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Calendar {
    /// A folder of .ics files. Empty is a plain month.
    pub dir: String,
    pub week_start: WeekStart,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WeekStart {
    Monday,
    Saturday,
    Sunday,
}

impl Default for Calendar {
    fn default() -> Self {
        Calendar { dir: String::new(), week_start: WeekStart::Sunday }
    }
}

impl Calendar {
    /// Monday is 0.
    pub fn first_weekday(&self) -> i64 {
        match self.week_start {
            WeekStart::Monday => 0,
            WeekStart::Saturday => 5,
            WeekStart::Sunday => 6,
        }
    }
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Blur {
    /// For the styles made for it: glass.
    Auto,
    On,
    Off,
}

/// Look and feel switches.
#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Ui {
    /// See-through, so a compositor that blurs layers has something to blur.
    pub blur: Blur,
    /// How solid backgrounds are. Unset is the style's own.
    pub opacity: Option<f64>,
    /// The island's own icons are solid, like the apps' glyphs. Off draws outlines,
    /// solid only for what's on.
    pub solid_icons: bool,
    /// The size of all of it: 0.75 to 2.
    pub scale: f64,
    /// Hovering the pill opens the panel. Off leaves it to a click or the pin key.
    pub hover_open: bool,
    /// Resting on the middle of the frame's right border opens the sidebar.
    pub hover_sidebar: bool,
    pub hover_open_ms: u64,
    pub hover_close_ms: u64,
}

impl Default for Ui {
    fn default() -> Self {
        Ui { blur: Blur::Auto, opacity: None, solid_icons: true, scale: 1.0, hover_open: true, hover_sidebar: true, hover_open_ms: 130, hover_close_ms: 280 }
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct Config {
    /// The island's shape. `island style` overrides it until this line changes.
    pub style: Style,
    pub font: String,
    pub icon_font: String,
    pub icon_style: IconStyle,
    pub clock: String,
    pub palette: String,
    /// Module order in the pill and the panel. See src/modules/mod.rs.
    pub pill: Vec<String>,
    pub panel: Vec<String>,
    /// The sidebar's sections, in order. Empty leaves it out. See src/modules/sidebar.rs.
    pub sidebar: Vec<String>,
    pub visualizer: Vec<crate::viz::Look>,
    pub calendar: Calendar,
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
    /// The search's last row looks up what you typed here, `{}` being the query. Empty hides it.
    pub web_search: String,
    pub screenshot_dir: String,
    pub keys: Keys,
    pub ui: Ui,
    pub pomodoro: Pomodoro,
    pub claude: Claude,
    /// Parts to leave out wherever they appear: module and tool names, plus "calc"
    /// and "web" for the search's sums and web row. What `install.sh --manual` writes.
    pub disable: Vec<String>,
    pub nightlight: crate::nightlight::Settings,
}

impl Default for Config {
    fn default() -> Self {
        let list = |l: &[&str]| l.iter().map(|s| s.to_string()).collect();
        Config {
            style: Style::Pill,
            font: "Adwaita Sans".into(),
            // Any Nerd Font has the icons, and this one is a sixth the size of Iosevka,
            // which cuts memory in half: HarfBuzz's tables scale with the font.
            icon_font: "JetBrainsMono Nerd Font Propo".into(),
            icon_style: IconStyle::Glyph,
            clock: "%I:%M %p".into(),
            palette: "~/.config/theme/generated/colors.toml".into(),
            pill: list(&["media", "workspaces", "claude", "timer", "network", "privacy", "battery", "clock"]),
            panel: list(&["header", "workspaces", "media", "timer", "notifications", "stats", "audio", "brightness", "nightlight", "status", "bluetooth"]),
            sidebar: list(&["media", "calendar", "controls", "notifications"]),
            visualizer: vec![crate::viz::Look::Bars, crate::viz::Look::Wave, crate::viz::Look::Ring],
            calendar: Calendar::default(),
            icons: HashMap::new(),
            commands: Commands::default(),
            custom: Vec::new(),
            tools: list(&["apps", "run", "windows", "files", "clipboard", "screenshot", "wifi", "bluetooth", "wallpaper", "power"]),
            tool: Vec::new(),
            notifications: true,
            notify: NotifyTimes::default(),
            wallpaper: Wallpaper::default(),
            terminal: "kitty".into(),
            web_search: "https://duckduckgo.com/?q={}".into(),
            screenshot_dir: "~/Pictures".into(),
            keys: Keys::default(),
            ui: Ui::default(),
            pomodoro: Pomodoro::default(),
            claude: Claude::default(),
            nightlight: Default::default(),
            disable: Vec::new(),
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        env::var_os("XDG_CONFIG_HOME").map_or(home().join(".config"), PathBuf::from).join("island/config.toml")
    }

    pub fn parse(src: &str) -> Result<Self, toml::de::Error> {
        let mut c: Config = toml::from_str(src)?;
        c.finish();
        Ok(c)
    }

    /// Takes out what's switched off, once everything that can switch it has had its say.
    pub fn finish(&mut self) {
        let off = self.disable.clone();
        for list in [&mut self.pill, &mut self.panel, &mut self.sidebar, &mut self.tools] {
            list.retain(|n| !off.contains(n));
        }
        if self.off("sidebar") {
            self.sidebar.clear();
        }
        if self.off("visualizer") {
            self.visualizer.clear();
        }
        if self.off("web") {
            self.web_search.clear();
        }
        if self.off("notifications") {
            self.notifications = false;
        }
        let ui = &mut self.ui;
        ui.scale = if ui.scale.is_finite() { ui.scale.clamp(crate::scale::MIN, crate::scale::MAX) } else { 1.0 };
        ui.opacity = ui.opacity.filter(|a| a.is_finite()).map(|a| a.clamp(0.3, 1.0));
    }

    pub fn off(&self, part: &str) -> bool {
        self.disable.iter().any(|d| d == part)
    }

    pub fn calc(&self) -> bool {
        !self.off("calc")
    }

    /// Something on screen uses it, so whatever feeds it has to run.
    pub fn uses(&self, part: &str) -> bool {
        [&self.pill, &self.panel, &self.sidebar, &self.tools].into_iter().flatten().any(|n| n == part)
    }

    /// config.toml as it's written. A broken one mustn't take the bar down with it: complain, use defaults.
    pub fn written() -> Self {
        let Ok(src) = fs::read_to_string(Self::path()) else { return Config::default() };
        match toml::from_str::<Config>(&src) {
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

    /// What's written, under what was chosen on the island.
    pub fn load(state: &State) -> Self {
        let mut c = Self::written();
        state.apply(&mut c);
        c.finish();
        c
    }

    /// How solid backgrounds are in this style.
    pub fn alpha(&self, style: Style) -> f64 {
        self.ui.opacity.unwrap_or(match () {
            _ if self.blurred(style) => 0.68,
            _ if style == Style::Glass => 0.88,
            _ => 1.0,
        })
    }

    pub fn blurred(&self, style: Style) -> bool {
        self.ui.blur == Blur::On || self.ui.blur == Blur::Auto && style == Style::Glass
    }
}

#[derive(Default, Debug, PartialEq)]
pub struct State {
    /// Output name -> pill centre x, pill top y. Absent means docked top-centre.
    pub pos: HashMap<String, (f64, f64)>,
    pub pinned: bool,
    pub tucked: bool,
    pub dnd: bool,
    /// Night light on, and the warmth the slider last chose.
    pub night: bool,
    pub warmth: Option<u32>,
    /// The style `island style` chose, and what config.toml said then.
    pub style: Option<(Style, Style)>,
    /// Other settings chosen on the island: key -> the choice, and what config.toml said then.
    pub chosen: BTreeMap<String, (String, String)>,
}

impl State {
    /// Each choice holds until the line it overrode changes.
    pub fn apply(&self, cfg: &mut Config) {
        for (key, (choice, base)) in &self.chosen {
            if crate::prefs::get(cfg, key).as_ref() == Some(base) {
                crate::prefs::put(cfg, key, choice);
            }
        }
    }

    /// `written` is config.toml with no choices over it. False, and no change, if it makes no sense.
    pub fn choose(&mut self, written: &Config, key: &str, value: &str) -> bool {
        let (Some(base), mut probe) = (crate::prefs::get(written, key), written.clone()) else { return false };
        if !crate::prefs::put(&mut probe, key, value) {
            return false;
        }
        match crate::prefs::get(&probe, key) {
            Some(choice) if choice != base => self.chosen.insert(key.to_owned(), (choice, base)),
            _ => self.chosen.remove(key),
        };
        true
    }

    /// The last word wins: the command's, until the config's line changes.
    pub fn style(&self, configured: Style) -> Style {
        match self.style {
            Some((chosen, base)) if base == configured => chosen,
            _ => configured,
        }
    }

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
                ["night"] => s.night = true,
                ["warmth", k] => s.warmth = k.parse().ok(),
                ["style", chosen, base] => s.style = Style::parse(chosen).zip(Style::parse(base)),
                ["set", key, choice, base] => {
                    s.chosen.insert(key.into(), (choice.into(), base.into()));
                }
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
        if self.night {
            out += "night\n";
        }
        if let Some(k) = self.warmth {
            out += &format!("warmth {k}\n");
        }
        if let Some((chosen, base)) = self.style {
            out += &format!("style {} {}\n", chosen.name(), base.name());
        }
        for (key, (choice, base)) in &self.chosen {
            out += &format!("set {key} {choice} {base}\n");
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
        let off = Config::parse("disable = [\"timer\", \"bluetooth\", \"web\", \"calc\"]").unwrap();
        assert!(!off.pill.contains(&"timer".into()) && !off.panel.contains(&"timer".into()) && !off.tools.contains(&"bluetooth".into()));
        assert!(off.web_search.is_empty() && !off.calc() && Config::default().calc());
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
    fn the_last_choice_of_style_wins() {
        let mut s = State::default();
        assert_eq!(s.style(Style::Notch), Style::Notch);
        s.style = Some((Style::Frame, Style::Pill));
        assert_eq!(s.style(Style::Pill), Style::Frame, "the command holds");
        assert_eq!(s.style(Style::Bar), Style::Bar, "until the config says something new");
        assert_eq!(State::parse(&s.render()).style, s.style);
        assert_eq!(Config::parse("style = \"notch\"").unwrap().style, Style::Notch);
    }

    #[test]
    fn state_round_trips_and_ignores_garbage() {
        let mut s = State { pinned: true, night: true, warmth: Some(3200), ..Default::default() };
        s.pos.insert("DP-2".into(), (640.5, 900.0));
        assert_eq!(State::parse(&s.render()), s);
        assert_eq!(State::parse("pos DP-2 x y\nnonsense\n\n"), State::default());
    }
}
